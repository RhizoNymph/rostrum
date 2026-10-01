//! [`run_stack_job`]: making pull requests into a stack, from a clone.
//!
//! The order is chosen so every early stop leaves something coherent:
//!
//! 1. **Fetch** the trunk and every head and base, and record where each
//!    remote branch is. Those oids are the leases.
//! 2. **Rebase** (only what needs it), each branch in its own detached
//!    scratch worktree, onto the rewritten branch below. No branch ref moves
//!    and nothing is pushed until *every* member has rebased cleanly, so a
//!    conflict leaves GitHub exactly as it was.
//! 3. **Push** each rewritten branch with `--force-with-lease` against the
//!    oid from step 1. A refused lease stops the job there.
//! 4. **Align local branches**: create missing ones, and move ones that were
//!    in sync with the remote, by compare-and-swap, never one that is
//!    checked out.
//! 5. **Link** the pull requests into a stack on GitHub (`gh stack link`),
//!    which also retargets each base to the branch below.
//! 6. **Track** the stack in the clone (`gh stack init`), when that is safe.
//!
//! Re-running after any stop is safe: a member whose branch already contains
//! the one below is not rebased again, and `rerere` replays any conflict
//! resolution recorded in a handed-off session.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rostrum_config::ConflictHandler;
use rostrum_core::{PlanMember, PrNumber, RepoId, StackMembers, StackNumber};
use rostrum_git::{
    BranchName, ConflictPolicy, GitError, Oid, Outcome, PushOutcome, RefExpectation, Remote,
    RemoteRef, Repo, Rev,
};
use rostrum_handoff::{HandoffError, PrMeta, hand_off, session_exists, session_name};

use crate::{
    error::StackOpError,
    gh::{GhRunner, GhStackCommand, StackView},
    job::{
        ExtendJob, LocalNote, LocalTracking, Progress, StackJob, StackOutcome, StackProgress,
        StackReport,
    },
    local_file::LocalStacks,
};

/// How long to give the tmux client; see rostrum-local's jobs.
const TMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// One plan member, with its names validated for git.
#[derive(Clone, Debug)]
struct Member {
    number: PrNumber,
    title: String,
    url: String,
    head: BranchName,
    base: BranchName,
}

/// Run a [`StackJob`] to an outcome: make a new stack.
///
/// `Err` means the job could not get as far as an outcome — the clone would
/// not open, a fetch failed, `gh` is missing — and, by rostrum-git's
/// invariant, nothing changed. Everything after the first push is reported
/// as a [`StackOutcome`] instead, because by then something has.
pub async fn run_stack_job<R: GhRunner>(
    job: StackJob,
    gh: &R,
    progress: &Progress,
) -> Result<StackOutcome, StackOpError> {
    let StackJob {
        clone,
        plan,
        handler,
        scratch_dir,
    } = job;
    let trunk = BranchName::new(plan.trunk.as_str())?;
    let chain = Chain {
        repo_id: plan.repo.clone(),
        root: trunk.clone(),
        root_number: None,
        members: members_of(plan.members())?,
        rewrite: plan.needs_rewrite(),
        target: Target::New {
            trunk,
            members: plan.as_stack().members,
        },
    };
    run_chain(chain, &clone, handler, &scratch_dir, gh, progress).await
}

/// Run an [`ExtendJob`] to an outcome: add pull requests to the top of a
/// stack GitHub already has.
///
/// The same pipeline as [`run_stack_job`], rooted at the stack's top member
/// instead of a trunk: additions that already chain off the top are linked
/// as they are; otherwise each is rebased onto the one below (the first onto
/// the top) and pushed with a lease. The stack's existing members are never
/// rebased or pushed. The final call is `gh stack link <stack> <pr>...`.
pub async fn run_extend_job<R: GhRunner>(
    job: ExtendJob,
    gh: &R,
    progress: &Progress,
) -> Result<StackOutcome, StackOpError> {
    let ExtendJob {
        clone,
        plan,
        handler,
        scratch_dir,
    } = job;
    let chain = Chain {
        repo_id: plan.repo.clone(),
        root: BranchName::new(plan.top.head.as_str())?,
        root_number: Some(plan.top.number),
        members: members_of(plan.additions())?,
        rewrite: plan.needs_rewrite(),
        target: Target::Extend {
            stack: plan.stack,
            additions: plan.addition_numbers().clone(),
        },
    };
    run_chain(chain, &clone, handler, &scratch_dir, gh, progress).await
}

/// What the pipeline links once its branches are in place.
enum Target {
    New {
        trunk: BranchName,
        members: StackMembers,
    },
    Extend {
        stack: StackNumber,
        additions: StackMembers,
    },
}

/// A line of branches to put on top of `root`, bottom first.
struct Chain {
    repo_id: RepoId,
    /// The branch the bottom member builds on: the trunk, or the top of the
    /// stack being extended. Never pushed.
    root: BranchName,
    /// The pull request `root` belongs to, when it is one.
    root_number: Option<PrNumber>,
    members: Vec<Member>,
    rewrite: bool,
    target: Target,
}

fn members_of(plan: &[PlanMember]) -> Result<Vec<Member>, GitError> {
    plan.iter()
        .map(|m| {
            Ok(Member {
                number: m.number,
                title: m.title.clone(),
                url: m.url.clone(),
                head: BranchName::new(m.head.as_str())?,
                base: BranchName::new(m.base.as_str())?,
            })
        })
        .collect()
}

/// Fetch, rebase, lease-push, align local branches, link, track.
async fn run_chain<R: GhRunner>(
    chain: Chain,
    clone: &Path,
    handler: Option<ConflictHandler>,
    scratch_dir: &Path,
    gh: &R,
    progress: &Progress,
) -> Result<StackOutcome, StackOpError> {
    let Chain {
        repo_id,
        root,
        root_number,
        members,
        rewrite,
        target,
    } = chain;

    if rewrite && let Some(handler) = &handler {
        preflight_handler(handler, &repo_id, &members).await?;
    }

    let policy = match handler {
        Some(_) => ConflictPolicy::Leave,
        None => ConflictPolicy::Abort,
    };
    let repo = Repo::open(clone).await?.with_conflict_policy(policy);

    // 1. Fetch, and record the leases.
    progress.send(StackProgress::Fetching);
    let mut to_fetch = BTreeSet::from([root.clone()]);
    for member in &members {
        to_fetch.insert(member.head.clone());
        to_fetch.insert(member.base.clone());
    }
    for branch in to_fetch {
        repo.fetch(&RemoteRef::origin(branch)).await?;
    }
    let root_oid = remote_oid(&repo, &root)
        .await?
        .ok_or_else(|| match root_number {
            Some(number) => StackOpError::MissingOnRemote {
                branch: root.to_string(),
                number,
            },
            None => StackOpError::MissingTrunk(root.to_string()),
        })?;
    let mut leases = Vec::with_capacity(members.len());
    let mut old_bases = Vec::with_capacity(members.len());
    for member in &members {
        let missing = |branch: &BranchName| StackOpError::MissingOnRemote {
            branch: branch.to_string(),
            number: member.number,
        };
        leases.push(
            remote_oid(&repo, &member.head)
                .await?
                .ok_or_else(|| missing(&member.head))?,
        );
        old_bases.push(
            remote_oid(&repo, &member.base)
                .await?
                .ok_or_else(|| missing(&member.base))?,
        );
    }

    // 2. Rebase what needs it; 3. push it.
    let mut results = leases.clone();
    let mut pushed = Vec::new();
    if rewrite {
        let scratch_dir = prepare_scratch_dir(scratch_dir)?;
        prune_scratch(&repo, &scratch_dir).await;

        for ix in 0..members.len() {
            let member = &members[ix];
            let (parent_branch, parent_oid) = match ix {
                0 => (&root, root_oid.clone()),
                _ => (&members[ix - 1].head, results[ix - 1].clone()),
            };
            let retargeted = &member.base != parent_branch;
            let parent_moved = ix > 0 && results[ix - 1] != leases[ix - 1];
            if !retargeted && !parent_moved {
                continue;
            }
            // Already built on the new parent and carrying nothing else: a
            // rerun after a partial push, or a branch someone rebased by
            // hand. Both halves matter. The branch must contain the parent,
            // and everything it was built on must be in the parent too —
            // otherwise moving `b` off `a` onto `main` would be skipped
            // because `b` happens to contain `main` as well as `a`.
            if repo.is_ancestor(&old_bases[ix], &parent_oid).await?
                && repo.is_ancestor(&parent_oid, &leases[ix]).await?
            {
                continue;
            }

            progress.send(StackProgress::Rebasing {
                number: member.number,
                step: ix + 1,
                of: members.len(),
            });
            let rebase = Rebase {
                repo: &repo,
                repo_id: &repo_id,
                scratch_dir: &scratch_dir,
                member,
                start: &leases[ix],
                onto: &parent_oid,
                upstream: &old_bases[ix],
                parent_branch,
                handler: handler.as_ref(),
            };
            match rebase.run().await? {
                Step::Rebased(oid) => results[ix] = oid,
                Step::Stopped(outcome) => return Ok(outcome),
            }
        }

        for (ix, member) in members.iter().enumerate() {
            if results[ix] == leases[ix] {
                continue;
            }
            progress.send(StackProgress::Pushing {
                number: member.number,
            });
            let outcome = repo
                .push_with_lease(&Remote::origin(), &member.head, &results[ix], &leases[ix])
                .await?;
            match outcome {
                PushOutcome::Rejected(reason) => {
                    return Ok(StackOutcome::PushRejected {
                        pushed,
                        number: member.number,
                        reason,
                    });
                }
                PushOutcome::Updated { .. } | PushOutcome::Created | PushOutcome::UpToDate => {
                    pushed.push(member.number);
                }
            }
        }
    }

    // 4. Local branches.
    let notes = align_local_branches(&repo, &members, &leases, &results).await;

    // 5. Link on GitHub.
    progress.send(StackProgress::Linking);
    let link = match &target {
        Target::New { trunk, members } => GhStackCommand::Link {
            base: trunk.clone(),
            members: members.clone(),
        },
        Target::Extend { stack, additions } => GhStackCommand::LinkExtend {
            stack: *stack,
            additions: additions.clone(),
        },
    };
    let linked = match gh.run(repo.root(), &repo_id, &link).await {
        Ok(output) => output.require_success(&link).map(drop),
        Err(err) => Err(err),
    };
    if let Err(err) = linked {
        if pushed.is_empty() {
            return Err(err);
        }
        return Ok(StackOutcome::LinkFailed {
            pushed,
            message: err.to_string(),
        });
    }

    // 6. Track in the clone.
    let report = |local| StackReport {
        rewritten: pushed.clone(),
        local,
        notes: notes.clone(),
    };
    match target {
        Target::New { trunk, .. } => {
            progress.send(StackProgress::Tracking);
            let local = track_locally(gh, &repo, &repo_id, &trunk, &members).await;
            tracing::info!(repo = %repo_id, rewritten = pushed.len(), ?local, "stack created");
            Ok(StackOutcome::Stacked(report(local)))
        }
        Target::Extend { stack, .. } => {
            // gh-stack has no "adopt these branches onto a tracked stack";
            // `gh stack sync` pulls GitHub's additions into local tracking.
            let local = LocalTracking::Skipped(
                "extended on GitHub only; run `gh stack sync` in the clone to track the new branches"
                    .into(),
            );
            tracing::info!(repo = %repo_id, stack = stack.get(), rewritten = pushed.len(), "stack extended");
            Ok(StackOutcome::Extended {
                stack,
                report: report(local),
            })
        }
    }
}

async fn remote_oid(repo: &Repo, branch: &BranchName) -> Result<Option<Oid>, GitError> {
    repo.resolve(&Rev::Remote(RemoteRef::origin(branch.clone())))
        .await
}

/// Everything about a handler that can be checked before git runs: the
/// template is usable, and no member's session is already running.
async fn preflight_handler(
    handler: &ConflictHandler,
    repo: &RepoId,
    members: &[Member],
) -> Result<(), StackOpError> {
    let probe = Path::new("/probe");
    rostrum_handoff::substitute(&handler.command, probe, probe)
        .map_err(|err| StackOpError::Handler(err.to_string()))?;
    for member in members {
        let session = session_name(repo, member.number);
        match session_exists(&session, TMUX_TIMEOUT).await {
            Ok(true) => {
                return Err(StackOpError::HandoffRunning {
                    session,
                    number: member.number,
                });
            }
            Ok(false) => {}
            Err(HandoffError::TmuxMissing { .. }) => {
                return Err(StackOpError::Handler("`tmux` is not installed".into()));
            }
            Err(err) => return Err(StackOpError::Handler(err.to_string())),
        }
    }
    Ok(())
}

fn prepare_scratch_dir(dir: &Path) -> Result<PathBuf, StackOpError> {
    std::fs::create_dir_all(dir).map_err(|source| StackOpError::ScratchDir {
        path: dir.to_path_buf(),
        source,
    })?;
    // `worktree list` reports resolved paths; comparing against an
    // unresolved one would never match.
    std::fs::canonicalize(dir).map_err(|source| StackOpError::ScratchDir {
        path: dir.to_path_buf(),
        source,
    })
}

/// Remove scratch worktrees an earlier run left behind, except any that
/// still hold a stopped rebase — those are a handoff someone may be working
/// in.
async fn prune_scratch(repo: &Repo, scratch_dir: &Path) {
    let Ok(entries) = repo.worktrees().await else {
        return;
    };
    for entry in entries
        .into_iter()
        .filter(|e| e.detached && e.path.starts_with(scratch_dir))
    {
        let busy = match Repo::open(&entry.path).await {
            Ok(scratch) => scratch
                .status()
                .await
                .map(|s| s.in_progress.is_some())
                .unwrap_or(true),
            // Gone from disk: nothing to protect.
            Err(_) => false,
        };
        if busy {
            continue;
        }
        if let Err(error) = repo.remove_scratch_worktree(&entry.path).await {
            tracing::debug!(%error, path = %entry.path.display(), "could not remove a stale scratch worktree");
        }
    }
}

enum Step {
    Rebased(Oid),
    Stopped(StackOutcome),
}

/// One member's rebase in its own scratch worktree.
struct Rebase<'a> {
    repo: &'a Repo,
    repo_id: &'a RepoId,
    scratch_dir: &'a Path,
    member: &'a Member,
    start: &'a Oid,
    onto: &'a Oid,
    upstream: &'a Oid,
    parent_branch: &'a BranchName,
    handler: Option<&'a ConflictHandler>,
}

impl Rebase<'_> {
    async fn run(self) -> Result<Step, StackOpError> {
        let path = scratch_path(self.scratch_dir, self.repo_id, self.member.number);
        let scratch = self.repo.add_scratch_worktree(&path, self.start).await?;
        let outcome = match scratch.rebase_scratch(self.onto, self.upstream).await {
            Ok(outcome) => outcome,
            Err(err) => {
                self.remove(&path).await;
                return Err(err.into());
            }
        };
        match outcome {
            Outcome::AlreadyUpToDate => {
                self.remove(&path).await;
                Ok(Step::Rebased(self.start.clone()))
            }
            Outcome::Completed { to, .. } => {
                self.remove(&path).await;
                Ok(Step::Rebased(to))
            }
            Outcome::Conflicted(conflict) => {
                let message = conflict.message().to_string();
                match (conflict.abort_target(), self.handler) {
                    (Some(target), Some(handler)) => {
                        let outcome = self.hand_off(&scratch, &path, handler, &message).await;
                        if !matches!(outcome, StackOutcome::HandedOff { .. }) {
                            let _ = scratch.abort(target).await;
                            self.remove(&path).await;
                        }
                        Ok(Step::Stopped(outcome))
                    }
                    _ => {
                        self.remove(&path).await;
                        Ok(Step::Stopped(StackOutcome::Conflicted {
                            number: self.member.number,
                            message,
                        }))
                    }
                }
            }
        }
    }

    /// Gather the context and start the handler. Anything short of a running
    /// session is reported as a plain conflict, which the caller aborts.
    async fn hand_off(
        &self,
        scratch: &Repo,
        path: &Path,
        handler: &ConflictHandler,
        message: &str,
    ) -> StackOutcome {
        let conflicted = |why: String| StackOutcome::Conflicted {
            number: self.member.number,
            message: format!("{message}; {why}, so the rebase was aborted"),
        };

        // The context compares against the local branch; make sure there is
        // one. Creating it at the remote tip is what step 4 would do anyway.
        let local = Rev::Local(self.member.head.clone());
        if matches!(self.repo.resolve(&local).await, Ok(None))
            && let Err(err) = self
                .repo
                .set_branch(&self.member.head, self.start, &RefExpectation::Absent)
                .await
        {
            return conflicted(format!("could not create the local branch ({err})"));
        }

        let target = RemoteRef::origin(self.parent_branch.clone());
        let mut context = match scratch.conflict_context(&self.member.head, &target).await {
            Ok(context) => context,
            Err(err) => return conflicted(format!("could not gather context ({err})")),
        };
        context.git_message = format!(
            "{message}\n\nThis is a stack arrangement: `{}` is being rebased onto the rewritten `{}` in a scratch worktree with a detached HEAD. Finish the rebase here (`git rebase --continue`); nothing has been pushed. Then run Arrange again in rostrum: git's rerere replays this resolution.",
            self.member.head, self.parent_branch
        );
        let meta = PrMeta {
            repo: self.repo_id.clone(),
            number: self.member.number,
            title: self.member.title.clone(),
            url: self.member.url.clone(),
            body: String::new(),
            head_ref: self.member.head.to_string(),
            base_ref: self.parent_branch.to_string(),
        };
        match hand_off(&meta, &context, path, &handler.command, TMUX_TIMEOUT).await {
            Ok(receipt) => StackOutcome::HandedOff {
                number: self.member.number,
                session: receipt.session,
                worktree: path.to_path_buf(),
            },
            Err(err) => conflicted(format!("the handler could not be started ({err})")),
        }
    }

    async fn remove(&self, path: &Path) {
        if let Err(error) = self.repo.remove_scratch_worktree(path).await {
            tracing::warn!(%error, path = %path.display(), "could not remove a scratch worktree");
        }
    }
}

/// A fresh, unique scratch path for one member.
fn scratch_path(dir: &Path, repo: &RepoId, number: PrNumber) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let slug: String = format!("{}-{}", repo.owner(), repo.name())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    dir.join(format!(
        "{slug}-{}-{}-{nonce}",
        number.0,
        std::process::id()
    ))
}

/// Create missing local branches, and move ones that matched the remote
/// before the rewrite. Never fails the job: every refusal becomes a note.
async fn align_local_branches(
    repo: &Repo,
    members: &[Member],
    leases: &[Oid],
    results: &[Oid],
) -> Vec<LocalNote> {
    let mut notes = Vec::new();
    for (ix, member) in members.iter().enumerate() {
        let branch = &member.head;
        let rewritten = results[ix] != leases[ix];
        let left = |reason: String| LocalNote::LeftBehind {
            branch: branch.clone(),
            reason,
        };
        match repo.resolve(&Rev::Local(branch.clone())).await {
            Err(err) => notes.push(left(err.to_string())),
            Ok(None) => {
                match repo
                    .set_branch(branch, &results[ix], &RefExpectation::Absent)
                    .await
                {
                    Ok(()) => notes.push(LocalNote::Created(branch.clone())),
                    Err(err) => notes.push(left(err.to_string())),
                }
            }
            Ok(Some(local)) if local == results[ix] => {}
            Ok(Some(local)) if rewritten && local == leases[ix] => {
                match repo
                    .set_branch(branch, &results[ix], &RefExpectation::At(local))
                    .await
                {
                    Ok(()) => notes.push(LocalNote::Moved(branch.clone())),
                    Err(GitError::CheckedOut { path, .. }) => notes.push(left(format!(
                        "checked out in `{}`; pull there to pick up the rewrite",
                        path.display()
                    ))),
                    Err(err) => notes.push(left(err.to_string())),
                }
            }
            Ok(Some(_)) if rewritten => notes.push(left(
                "has commits that were not on the remote; left as it was".into(),
            )),
            Ok(Some(_)) => {}
        }
    }
    notes
}

/// Whether `gh stack init` can run, and the reason when it cannot.
enum Tracking {
    Clear,
    Already,
    Blocked(String),
}

async fn tracking_precondition(repo: &Repo, trunk: &BranchName, members: &[Member]) -> Tracking {
    let names: Vec<String> = members.iter().map(|m| m.head.to_string()).collect();
    match LocalStacks::load(repo.git_dir()) {
        Err(err) => return Tracking::Blocked(err.to_string()),
        Ok(LocalStacks::NewerSchema(version)) => {
            return Tracking::Blocked(format!(
                "the clone's gh-stack file is schema {version}, newer than rostrum reads"
            ));
        }
        Ok(file) => {
            if let Some(stack) = names.iter().find_map(|n| file.stack_with_branch(n)) {
                let same = stack.trunk == trunk.as_str()
                    && stack.branches.iter().map(|b| &b.name).eq(names.iter());
                return if same {
                    Tracking::Already
                } else {
                    Tracking::Blocked(format!(
                        "a member is already tracked in another local stack (onto `{}`)",
                        stack.trunk
                    ))
                };
            }
        }
    }

    for member in members {
        match repo.resolve(&Rev::Local(member.head.clone())).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Tracking::Blocked(format!("`{}` has no local branch", member.head));
            }
            Err(err) => return Tracking::Blocked(err.to_string()),
        }
    }

    let status = match repo.status().await {
        Ok(status) => status,
        Err(err) => return Tracking::Blocked(err.to_string()),
    };
    if let Some(state) = status.in_progress {
        return Tracking::Blocked(format!("{} is in progress in the clone", state.describe()));
    }
    if status.worktree.staged > 0 || status.worktree.unstaged > 0 {
        return Tracking::Blocked(
            "the clone has uncommitted changes, and `gh stack init` checks out the top branch"
                .into(),
        );
    }
    let top = &members[members.len() - 1].head;
    match repo.worktrees().await {
        Ok(entries) => {
            if let Some(entry) = entries
                .iter()
                .find(|e| e.branch.as_ref() == Some(top) && e.path != repo.root())
            {
                return Tracking::Blocked(format!(
                    "the top branch `{top}` is checked out in `{}`, and `gh stack init` checks it out in the clone",
                    entry.path.display()
                ));
            }
        }
        Err(err) => return Tracking::Blocked(err.to_string()),
    }
    Tracking::Clear
}

async fn track_locally<R: GhRunner>(
    gh: &R,
    repo: &Repo,
    repo_id: &RepoId,
    trunk: &BranchName,
    members: &[Member],
) -> LocalTracking {
    match tracking_precondition(repo, trunk, members).await {
        Tracking::Already => return LocalTracking::Tracked,
        Tracking::Blocked(reason) => return LocalTracking::Skipped(reason),
        Tracking::Clear => {}
    }

    let init = GhStackCommand::Init {
        base: trunk.clone(),
        branches: members.iter().map(|m| m.head.clone()).collect(),
    };
    if let Err(err) = gh
        .run(repo.root(), repo_id, &init)
        .await
        .and_then(|out| out.require_success(&init))
    {
        return LocalTracking::Skipped(err.to_string());
    }

    let view = GhStackCommand::ViewJson;
    let parsed = gh
        .run(repo.root(), repo_id, &view)
        .await
        .and_then(|out| out.require_success(&view))
        .and_then(|out| StackView::parse(&out.stdout));
    match parsed {
        Ok(view) => {
            let expected: Vec<String> = members.iter().map(|m| m.head.to_string()).collect();
            if view.trunk == trunk.as_str() && view.branch_names() == expected {
                LocalTracking::Tracked
            } else {
                LocalTracking::Skipped(format!(
                    "`gh stack view` shows {} onto `{}` instead",
                    view.branch_names().join(" ← "),
                    view.trunk
                ))
            }
        }
        Err(err) => LocalTracking::Skipped(format!("could not confirm local tracking: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_paths_are_unique_and_slugged() {
        let dir = Path::new("/scratch");
        let repo = RepoId::new("Owner.x", "re po");
        let a = scratch_path(dir, &repo, PrNumber(4));
        let b = scratch_path(dir, &repo, PrNumber(4));
        assert_ne!(a, b);
        let name = a.file_name().and_then(|n| n.to_str()).expect("utf-8");
        assert!(name.starts_with("Owner-x-re-po-4-"), "{name}");
        assert!(a.starts_with(dir));
    }
}
