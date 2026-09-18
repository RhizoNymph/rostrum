//! The handle everything else hangs off.
//!
//! [`Repo`] is cheap to clone — an [`Arc`] around a few paths and the timeout
//! budget — so a UI can hand one to every task that needs it, the same way
//! `GitHubClient` is cloned.
//!
//! Opening a repository resolves its paths **once**. After that, asking whether
//! a rebase is in progress is seven `stat` calls rather than a subprocess,
//! which matters when the answer is wanted on every redraw.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rostrum_core::Divergence;

use crate::{
    command::{self, CommandKind, Run, Timeouts},
    error::GitError,
    fetch::{FetchOutcome, classify_fetch, parse_fetch_porcelain},
    outcome::{AbortTarget, Conflict, Outcome, RunReport, classify_run},
    preflight::{Autostash, Operation, Preflight, blockers},
    refs::{BranchName, RemoteRef, Rev},
    status::{RepoStatus, StateFiles, in_progress, parse_left_right_count, parse_status_v2},
};

/// Absolute paths to the per-worktree files that mark a stopped operation.
///
/// Resolved with `rev-parse --git-path`, never by joining onto `.git/`. In a
/// multi-worktree checkout — which rostrum's own repository is — `.git` in the
/// work tree is a *file*, the real directory lives under
/// `<common>/worktrees/<name>/`, and these files are per-worktree. Building the
/// path by hand would look for a merge in the wrong worktree forever.
#[derive(Clone, Debug)]
struct StatePaths {
    rebase_merge: PathBuf,
    rebase_apply: PathBuf,
    rebase_applying: PathBuf,
    merge_head: PathBuf,
    cherry_pick_head: PathBuf,
    revert_head: PathBuf,
    bisect_log: PathBuf,
}

impl StatePaths {
    /// The `--git-path` arguments, in the order the fields are read back.
    const SPECS: [&'static str; 7] = [
        "rebase-merge",
        "rebase-apply",
        "rebase-apply/applying",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
    ];

    fn from_lines(lines: &[&str]) -> Result<Self, GitError> {
        let [
            rebase_merge,
            rebase_apply,
            rebase_applying,
            merge_head,
            cherry_pick_head,
            revert_head,
            bisect_log,
        ] = lines
        else {
            return Err(GitError::Parse {
                what: "rev-parse --git-path output",
                line: lines.join(" "),
            });
        };
        Ok(Self {
            rebase_merge: PathBuf::from(rebase_merge),
            rebase_apply: PathBuf::from(rebase_apply),
            rebase_applying: PathBuf::from(rebase_applying),
            merge_head: PathBuf::from(merge_head),
            cherry_pick_head: PathBuf::from(cherry_pick_head),
            revert_head: PathBuf::from(revert_head),
            bisect_log: PathBuf::from(bisect_log),
        })
    }

    /// Which of them exist right now.
    fn probe(&self) -> Result<StateFiles, GitError> {
        Ok(StateFiles {
            rebase_merge: exists(&self.rebase_merge)?,
            rebase_apply: exists(&self.rebase_apply)?,
            rebase_applying: exists(&self.rebase_applying)?,
            merge_head: exists(&self.merge_head)?,
            cherry_pick_head: exists(&self.cherry_pick_head)?,
            revert_head: exists(&self.revert_head)?,
            bisect_log: exists(&self.bisect_log)?,
        })
    }
}

/// "Not there" is the ordinary answer; anything else is a real failure and must
/// not be read as "no rebase in progress".
fn exists(path: &Path) -> Result<bool, GitError> {
    match std::fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(GitError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[derive(Debug)]
struct Inner {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
    state: StatePaths,
    timeouts: Timeouts,
}

/// A git work tree rostrum can read and act on.
#[derive(Clone, Debug)]
pub struct Repo {
    inner: Arc<Inner>,
}

impl Repo {
    /// Open the repository containing `path`.
    ///
    /// One `rev-parse` resolves the work tree root, the git directory, and the
    /// common directory together. `--path-format=absolute` is required: without
    /// it `--git-dir` comes back as the relative string `.git`, which stops
    /// being meaningful the moment it is used from anywhere but the root.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, GitError> {
        Self::open_with(path, Timeouts::default()).await
    }

    pub async fn open_with(path: impl AsRef<Path>, timeouts: Timeouts) -> Result<Self, GitError> {
        let path = path.as_ref();
        let run = command::run(
            path,
            &strings([
                "rev-parse",
                "--path-format=absolute",
                "--show-toplevel",
                "--git-dir",
                "--git-common-dir",
                "--is-bare-repository",
            ]),
            CommandKind::Read,
            &timeouts,
        )
        .await?;

        if !run.success {
            return Err(GitError::NotARepository {
                path: path.to_path_buf(),
                stderr: run.stderr.trim().to_string(),
            });
        }

        let lines: Vec<&str> = run.stdout.lines().map(str::trim).collect();
        let [root, git_dir, common_dir, bare] = lines.as_slice() else {
            return Err(GitError::Parse {
                what: "rev-parse output",
                line: run.stdout.trim().to_string(),
            });
        };

        let root = PathBuf::from(root);
        if *bare == "true" {
            return Err(GitError::Bare { path: root });
        }

        // Resolved against the discovered root, so the paths are the ones this
        // handle will use for every later call.
        let mut args = vec![
            "rev-parse".to_string(),
            "--path-format=absolute".to_string(),
        ];
        for spec in StatePaths::SPECS {
            args.push("--git-path".to_string());
            args.push(spec.to_string());
        }
        let state_run = command::run(&root, &args, CommandKind::Read, &timeouts)
            .await?
            .require_success("rev-parse --git-path")?;
        let state =
            StatePaths::from_lines(&state_run.stdout.lines().map(str::trim).collect::<Vec<_>>())?;

        Ok(Self {
            inner: Arc::new(Inner {
                root,
                git_dir: PathBuf::from(*git_dir),
                common_dir: PathBuf::from(*common_dir),
                state,
                timeouts,
            }),
        })
    }

    /// The same repository with a different timeout budget.
    pub fn with_timeouts(&self, timeouts: Timeouts) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: self.inner.root.clone(),
                git_dir: self.inner.git_dir.clone(),
                common_dir: self.inner.common_dir.clone(),
                state: self.inner.state.clone(),
                timeouts,
            }),
        }
    }

    /// The absolute work tree root.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// The git directory for *this* worktree.
    pub fn git_dir(&self) -> &Path {
        &self.inner.git_dir
    }

    /// The git directory shared by every worktree of this repository.
    pub fn common_dir(&self) -> &Path {
        &self.inner.common_dir
    }

    pub fn timeouts(&self) -> Timeouts {
        self.inner.timeouts
    }

    /// Branch, dirtiness, conflicts, upstream divergence, and any stopped
    /// operation.
    ///
    /// One spawn plus seven `stat` calls. Doing it in one `status` is not only
    /// cheaper than four commands, it is the only way the four answers are
    /// guaranteed to describe the same instant.
    pub async fn status(&self) -> Result<RepoStatus, GitError> {
        let run = self
            .run(
                &strings([
                    // Pinned on, because `status.aheadBehind=false` in a user's
                    // config makes git print `+? -?` instead of the counts.
                    "-c",
                    "status.aheadBehind=true",
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=normal",
                    // A submodule whose own worktree is dirty does not stop a
                    // rebase of the superproject, so it must not count as dirt.
                    "--ignore-submodules=dirty",
                ]),
                CommandKind::Read,
            )
            .await?
            .require_success("status")?;

        let (head, worktree) = parse_status_v2(&run.stdout)?;
        Ok(RepoStatus {
            head,
            worktree,
            in_progress: in_progress(self.inner.state.probe()?),
        })
    }

    /// Whether a ref exists, without resolving it.
    ///
    /// `show-ref --verify` does no revision parsing at all, which is the point:
    /// `rev-parse --verify main` would happily accept a tag named `main`, or
    /// `main@{1}`, and answer yes about something that is not the branch.
    pub async fn ref_exists(&self, rev: &Rev) -> Result<bool, GitError> {
        let arg = rev.as_arg();
        let run = self
            .run(
                &strings(["show-ref", "--verify", "--quiet", "--", arg.as_str()]),
                CommandKind::Read,
            )
            .await?;
        match run.code {
            Some(0) => Ok(true),
            // Exit 1 is git's answer to "no such ref", not a failure to ask.
            Some(1) => Ok(false),
            _ => Err(run
                .require_success("show-ref")
                .expect_err("a non-zero, non-one exit is a failure")),
        }
    }

    /// How far `left` is ahead of and behind `right`.
    ///
    /// Left is ahead, right is behind, matching
    /// [`Divergence`](rostrum_core::Divergence)'s own convention. Both sides are
    /// fully qualified by [`Rev::as_arg`], because a bare name resolves against
    /// `refs/tags/` before `refs/heads/`.
    pub async fn divergence(&self, left: &Rev, right: &Rev) -> Result<Divergence, GitError> {
        let range = format!("{}...{}", left.as_arg(), right.as_arg());
        let run = self
            .run(
                &strings(["rev-list", "--left-right", "--count", range.as_str(), "--"]),
                CommandKind::Read,
            )
            .await?
            .require_success("rev-list")?;
        parse_left_right_count(&run.stdout)
    }

    /// What stands between the current state and `operation`.
    ///
    /// `expected` is the branch the caller believes is checked out; pass `None`
    /// for an operation that acts on whatever HEAD is on. The result is for
    /// display: a caller that ignores it and calls the operation anyway gets
    /// [`GitError::Refused`] rather than a half-done repository.
    pub async fn preflight(
        &self,
        operation: Operation,
        expected: Option<&BranchName>,
        autostash: Autostash,
    ) -> Result<Preflight, GitError> {
        let status = self.status().await?;
        Ok(Preflight::new(
            operation,
            blockers(operation, &status, expected, autostash),
        ))
    }

    /// Update one tracking ref from its remote.
    ///
    /// The refspec is explicit and forced. `git fetch origin main` writes only
    /// `FETCH_HEAD`, leaving `refs/remotes/origin/main` at whatever it was, so
    /// every divergence computed afterwards would be stale — and without the
    /// leading `+` a force-pushed pull request branch is rejected outright.
    ///
    /// `--no-write-fetch-head` keeps the tracking ref the single source of
    /// truth; `--no-tags` stops an unrelated tag fetch from dominating the
    /// round trip.
    pub async fn fetch(&self, remote_ref: &RemoteRef) -> Result<FetchOutcome, GitError> {
        let refspec = remote_ref.fetch_refspec();
        let run = self
            .run(
                &strings([
                    // A credential helper that wants to ask a question would
                    // otherwise hold the connection open until the timeout.
                    "-c",
                    "credential.interactive=false",
                    "-c",
                    "gc.auto=0",
                    "fetch",
                    "--porcelain",
                    "--verbose",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "--no-write-fetch-head",
                    "--",
                    remote_ref.remote.as_str(),
                    refspec.as_str(),
                ]),
                CommandKind::Network,
            )
            .await?;

        let lines = if run.success {
            parse_fetch_porcelain(&run.stdout)?
        } else {
            Vec::new()
        };
        classify_fetch(
            &lines,
            &remote_ref.tracking_ref(),
            run.success,
            run.code,
            &run.stderr,
        )
    }

    /// Fetch, then rebase the current branch onto the freshly updated tracking
    /// ref.
    ///
    /// Deliberately **not** `git pull --rebase`, which rebases onto
    /// `FETCH_HEAD` and leaves `refs/remotes/<remote>/<branch>` stale — so the
    /// divergence rostrum shows immediately afterwards would still claim the
    /// branch is behind. `git pull` also derives its upstream from
    /// `branch.*.merge` and re-reads four more configuration knobs on the way,
    /// none of which rostrum controls.
    pub async fn pull_rebase(
        &self,
        upstream: &RemoteRef,
        autostash: Autostash,
    ) -> Result<Outcome, GitError> {
        match self.fetch(upstream).await? {
            FetchOutcome::Gone => {
                return Err(GitError::Failed {
                    command: Operation::PullRebase.as_str().to_string(),
                    code: None,
                    stderr: format!(
                        "`{}` no longer exists on `{}`",
                        upstream.branch, upstream.remote
                    ),
                });
            }
            FetchOutcome::UpToDate | FetchOutcome::Updated { .. } => {}
        }

        self.rebase(Operation::PullRebase, None, upstream, autostash)
            .await
    }

    /// Merge `base`'s tracking ref into `branch`, which must be the branch
    /// currently checked out.
    ///
    /// Does not fetch; call [`Repo::fetch`] first if the tracking ref might be
    /// stale. Neither `--ff-only` nor `--no-ff` is passed: the first would fail
    /// the ordinary diverged case this exists to resolve, and the second would
    /// manufacture an empty merge commit when the branch is merely behind.
    pub async fn merge_from(
        &self,
        branch: &BranchName,
        base: &RemoteRef,
        autostash: Autostash,
    ) -> Result<Outcome, GitError> {
        let target = base.tracking_ref();
        let args = strings([
            "-c",
            "gc.auto=0",
            "merge",
            "--no-edit",
            "--no-stat",
            autostash.as_flag(),
            "--end-of-options",
            target.as_str(),
        ]);
        self.write(Operation::Merge, Some(branch), autostash, &target, args)
            .await
    }

    /// Rebase `branch`, which must be the branch currently checked out, onto
    /// `base`'s tracking ref.
    ///
    /// Does not fetch; see [`Repo::pull_rebase`] for the combined form.
    pub async fn rebase_onto(
        &self,
        branch: &BranchName,
        base: &RemoteRef,
        autostash: Autostash,
    ) -> Result<Outcome, GitError> {
        self.rebase(Operation::Rebase, Some(branch), base, autostash)
            .await
    }

    /// Abort a stopped operation.
    ///
    /// The only way to obtain an [`AbortTarget`] is
    /// [`Conflict::abort_target`](crate::Conflict::abort_target), so
    /// `merge --abort` in the middle of a rebase is not something a caller can
    /// express.
    pub async fn abort(&self, target: AbortTarget) -> Result<(), GitError> {
        self.run(
            &strings(["-c", "gc.auto=0", target.subcommand(), "--abort"]),
            CommandKind::Write,
        )
        .await?
        .require_success(&format!("{} --abort", target.subcommand()))?;
        Ok(())
    }

    /// The shared body of [`Repo::rebase_onto`] and [`Repo::pull_rebase`].
    ///
    /// The **one-argument** form is not an accident. `git rebase <upstream>
    /// <branch>` checks `<branch>` out first, which is a HEAD move rostrum
    /// never promised and cannot undo if the rebase then stops.
    ///
    /// * `--no-update-refs` — a user's `rebase.updateRefs=true` would move every
    ///   *other* local branch pointing into the rebased range, silently
    ///   rewriting work the caller never named.
    /// * `--no-fork-point` — `--fork-point` consults the reflog, so the same
    ///   command gives different answers on two machines with the same history.
    /// * `--empty=drop` — a commit whose changes are already upstream is
    ///   dropped rather than stopping the rebase to ask.
    async fn rebase(
        &self,
        operation: Operation,
        branch: Option<&BranchName>,
        base: &RemoteRef,
        autostash: Autostash,
    ) -> Result<Outcome, GitError> {
        let target = base.tracking_ref();
        let args = strings([
            "-c",
            "gc.auto=0",
            "rebase",
            autostash.as_flag(),
            "--no-fork-point",
            "--empty=drop",
            "--no-update-refs",
            "--end-of-options",
            target.as_str(),
        ]);
        self.write(operation, branch, autostash, &target, args)
            .await
    }

    /// Pre-flight, run, re-read, classify, apply the conflict policy.
    async fn write(
        &self,
        operation: Operation,
        expected: Option<&BranchName>,
        autostash: Autostash,
        target: &str,
        args: Vec<String>,
    ) -> Result<Outcome, GitError> {
        let before = self.status().await?;
        if let Some(blocker) = blockers(operation, &before, expected, autostash)
            .into_iter()
            .next()
        {
            return Err(GitError::Refused { operation, blocker });
        }

        let run = self.run(&args, CommandKind::Write).await?;
        // The exit code is not consulted for the verdict; the repository is.
        let after = self.status().await?;

        let outcome = classify_run(&RunReport {
            operation,
            target,
            success: run.success,
            code: run.code,
            message: &run.message(),
            stderr: &run.stderr,
            before: before.oid(),
            after: after.oid(),
            in_progress: after.in_progress,
            conflicted: after.worktree.conflicted,
        })?;

        match outcome {
            Outcome::Conflicted(conflict) => {
                Ok(Outcome::Conflicted(self.on_conflict(conflict).await))
            }
            settled => Ok(settled),
        }
    }

    /// **Conflict policy: auto-abort.**
    ///
    /// When a rebase or merge stops on a conflict, rostrum runs the matching
    /// `--abort` and reports the conflict with git's own message, leaving the
    /// repository exactly as it was before the button was pressed.
    ///
    /// The reasoning: rostrum is a review tool, not an editor. It has no
    /// conflict resolution UI, and a user who discovers a half-finished rebase
    /// the next time they open a terminal has been handed a problem they did
    /// not ask for. Reporting "this would conflict" and changing nothing is the
    /// honest answer to a button press.
    ///
    /// A [`Conflict::AutostashPop`] is deliberately **not** aborted. There the
    /// operation already succeeded and there is no sequencer state, so
    /// `--abort` would simply fail; the user's changes are safe in the stash and
    /// the right thing is to say so.
    ///
    /// This is a decision, not a law — a future version with a conflict editor
    /// would leave the state in place instead — which is why it lives in one
    /// named place rather than being spread through the call sites.
    async fn on_conflict(&self, mut conflict: Conflict) -> Conflict {
        let Some(target) = conflict.abort_target() else {
            return conflict;
        };
        match self.abort(target).await {
            Ok(()) => conflict.mark_aborted(true),
            Err(error) => {
                // The repository is *not* back to where it started, and the
                // caller can see that from `abort_target()` still being `Some`.
                tracing::warn!(
                    %error,
                    target = target.subcommand(),
                    "could not abort after a conflict; sequencer state is still on disk"
                );
            }
        }
        conflict
    }

    async fn run(&self, args: &[String], kind: CommandKind) -> Result<Run, GitError> {
        command::run(&self.inner.root, args, kind, &self.inner.timeouts).await
    }
}

fn strings<const N: usize>(args: [&str; N]) -> Vec<String> {
    args.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These are per-worktree files. In rostrum's own repository the work tree
    /// root's `.git` is a *file*, so joining `root/.git/MERGE_HEAD` by hand
    /// would look for a merge that could never be there.
    #[test]
    fn the_state_specs_cover_every_operation_the_table_names() {
        assert_eq!(
            StatePaths::SPECS,
            [
                "rebase-merge",
                "rebase-apply",
                "rebase-apply/applying",
                "MERGE_HEAD",
                "CHERRY_PICK_HEAD",
                "REVERT_HEAD",
                "BISECT_LOG",
            ]
        );
    }

    #[test]
    fn state_paths_are_read_back_in_the_order_they_were_asked_for() {
        let lines: Vec<&str> = vec![
            "/r/.git/rebase-merge",
            "/r/.git/rebase-apply",
            "/r/.git/rebase-apply/applying",
            "/r/.git/MERGE_HEAD",
            "/r/.git/CHERRY_PICK_HEAD",
            "/r/.git/REVERT_HEAD",
            "/r/.git/BISECT_LOG",
        ];
        let paths = StatePaths::from_lines(&lines).expect("parses");
        assert_eq!(paths.rebase_merge, PathBuf::from("/r/.git/rebase-merge"));
        assert_eq!(
            paths.rebase_applying,
            PathBuf::from("/r/.git/rebase-apply/applying")
        );
        assert_eq!(paths.bisect_log, PathBuf::from("/r/.git/BISECT_LOG"));
    }

    /// A short read would otherwise shift every path by one and silently look
    /// for `MERGE_HEAD` at the cherry-pick path.
    #[test]
    fn a_short_git_path_listing_is_rejected() {
        assert!(StatePaths::from_lines(&["/r/.git/rebase-merge"]).is_err());
        assert!(StatePaths::from_lines(&[]).is_err());
    }

    /// A missing file is the normal answer; anything else must not be read as
    /// "no operation in progress".
    #[test]
    fn a_missing_state_file_is_not_an_error() {
        assert!(
            !exists(Path::new("/nonexistent/rostrum/MERGE_HEAD")).expect("absence is an answer")
        );
    }
}
