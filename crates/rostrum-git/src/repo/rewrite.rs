//! The writes behind arranging pull requests into a stack.
//!
//! Everything else in this crate acts on a branch where the user has it
//! checked out. Stacking pull requests that were not built on each other
//! means rebasing each branch onto the one below it, and doing that in the
//! user's worktrees would move their checkouts under them. So these
//! primitives work *beside* the user's state instead:
//!
//! - [`Repo::add_scratch_worktree`] checks a commit out **detached** in a new
//!   worktree, so no branch is touched while commits are replayed.
//! - [`Repo::rebase_scratch`] replays `upstream..HEAD` onto a commit there,
//!   with `rerere` on, so a conflict someone resolved once is reapplied
//!   automatically the next time.
//! - [`Repo::push_with_lease`] publishes the result — the single push in
//!   rostrum, always leased; see [`crate::push`].
//! - [`Repo::set_branch`] moves a local branch only from an exact expected
//!   value, and never one a worktree has checked out.
//!
//! The invariant of the crate still holds for each call: `Err` means nothing
//! changed.

use std::path::Path;

use crate::{
    command::{self, CommandKind},
    error::GitError,
    outcome::{Outcome, RunReport, classify_run},
    preflight::Operation,
    push::{PushOutcome, classify_push, push_args},
    refs::{BranchName, Oid, Remote, Rev},
    status::{Head, InProgress},
};

use super::{Repo, strings};

/// What a branch must be before [`Repo::set_branch`] moves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefExpectation {
    /// The branch must not exist yet.
    Absent,
    /// The branch must point exactly here.
    At(Oid),
}

/// How many `rebase --continue` rounds a rerere-resolved rebase may take.
/// Each round replays at least one commit, so this is a bound on commits, set
/// far above any pull request; it exists so a misbehaving rebase cannot spin.
const MAX_CONTINUES: usize = 10_000;

impl Repo {
    /// Resolve `rev` to the commit it names, or `None` if it does not exist.
    ///
    /// `--end-of-options` and the `^{commit}` peel keep a ref that happens to
    /// name a tag, or an argument that looks like an option, from changing
    /// the answer.
    pub async fn resolve(&self, rev: &Rev) -> Result<Option<Oid>, GitError> {
        let spec = format!("{}^{{commit}}", rev.as_arg());
        let run = self
            .run(
                &strings([
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "--end-of-options",
                    spec.as_str(),
                ]),
                CommandKind::Read,
            )
            .await?;
        match run.code {
            Some(0) => Oid::parse(run.stdout.trim()).map(Some),
            Some(1) => Ok(None),
            _ => Err(run
                .require_success("rev-parse")
                .expect_err("a non-zero, non-one exit is a failure")),
        }
    }

    /// Whether `ancestor` is reachable from `descendant` (or equal to it).
    pub async fn is_ancestor(&self, ancestor: &Oid, descendant: &Oid) -> Result<bool, GitError> {
        let run = self
            .run(
                &strings([
                    "merge-base",
                    "--is-ancestor",
                    ancestor.as_str(),
                    descendant.as_str(),
                ]),
                CommandKind::Read,
            )
            .await?;
        match run.code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(run
                .require_success("merge-base --is-ancestor")
                .expect_err("a non-zero, non-one exit is a failure")),
        }
    }

    /// Point `refs/heads/<branch>` at `new`, but only if it is currently what
    /// `expected` says, and only if no worktree has it checked out.
    ///
    /// `update-ref` with an old value is a compare-and-swap: if anything moved
    /// the branch since the caller looked, git refuses and nothing changes.
    pub async fn set_branch(
        &self,
        branch: &BranchName,
        new: &Oid,
        expected: &RefExpectation,
    ) -> Result<(), GitError> {
        if let Some(entry) = self
            .worktrees()
            .await?
            .into_iter()
            .find(|entry| entry.branch.as_ref() == Some(branch))
        {
            return Err(GitError::CheckedOut {
                branch: branch.to_string(),
                path: entry.path,
            });
        }

        let old = match expected {
            // The empty string is update-ref's "must not exist".
            RefExpectation::Absent => String::new(),
            RefExpectation::At(oid) => oid.to_string(),
        };
        let qualified = branch.qualified();
        self.run(
            &strings([
                "update-ref",
                "-m",
                "rostrum: stack",
                "--no-deref",
                "--",
                qualified.as_str(),
                new.as_str(),
                old.as_str(),
            ]),
            CommandKind::Write,
        )
        .await?
        .require_success("update-ref")?;
        Ok(())
    }

    /// Check `at` out detached in a new worktree at `path`, and open it with
    /// this handle's timeouts and conflict policy.
    ///
    /// `path` must not exist yet; git refuses otherwise, which is the right
    /// answer for a directory someone else may own.
    pub async fn add_scratch_worktree(&self, path: &Path, at: &Oid) -> Result<Repo, GitError> {
        let target = path.display().to_string();
        self.run(
            &strings([
                "worktree",
                "add",
                "--detach",
                "--quiet",
                "--",
                target.as_str(),
                at.as_str(),
            ]),
            CommandKind::Write,
        )
        .await?
        .require_success("worktree add")?;
        Repo::open_full(path, self.inner.timeouts, self.inner.conflict_policy).await
    }

    /// Remove a worktree this crate added, even with a stopped rebase in it.
    ///
    /// Forced because a scratch worktree is only ever removed by the code that
    /// made it, and a rebase that was aborted or abandoned there leaves files
    /// git counts as modifications.
    pub async fn remove_scratch_worktree(&self, path: &Path) -> Result<(), GitError> {
        let target = path.display().to_string();
        self.run(
            &strings(["worktree", "remove", "--force", "--", target.as_str()]),
            CommandKind::Write,
        )
        .await?
        .require_success("worktree remove")?;
        Ok(())
    }

    /// Replay the commits in `upstream..HEAD` onto `onto`, in a scratch
    /// worktree whose HEAD is detached.
    ///
    /// `upstream` is the commit the pull request's own work starts after — its
    /// old base — so exactly the pull request's commits move, whatever the old
    /// and new bases share.
    ///
    /// `rerere` is switched on for the command. When it can replay every
    /// conflicted path from a resolution recorded earlier, the rebase is
    /// continued here rather than stopped: a user who resolved a conflict in a
    /// handed-off session, then ran the arrangement again, should not have to
    /// resolve it twice. A conflict rerere cannot settle is handled by this
    /// handle's [`ConflictPolicy`](super::ConflictPolicy) like any other.
    pub async fn rebase_scratch(&self, onto: &Oid, upstream: &Oid) -> Result<Outcome, GitError> {
        let before = self.status().await?;
        let reason = match (&before.head, before.in_progress) {
            (_, Some(state)) => Some(format!("{} is in progress", state.describe())),
            (Head::Detached { .. }, None)
                if before.worktree.staged == 0 && before.worktree.unstaged == 0 =>
            {
                None
            }
            (Head::Detached { .. }, None) => Some("it has local changes".to_string()),
            _ => Some("HEAD is not detached".to_string()),
        };
        if let Some(reason) = reason {
            return Err(GitError::NotScratch {
                path: self.root().to_path_buf(),
                reason,
            });
        }

        let args = strings([
            "-c",
            "gc.auto=0",
            "-c",
            "rerere.enabled=true",
            "-c",
            "rerere.autoUpdate=true",
            "rebase",
            "--no-autostash",
            "--no-fork-point",
            "--empty=drop",
            "--no-update-refs",
            "--onto",
            onto.as_str(),
            "--end-of-options",
            upstream.as_str(),
        ]);
        let mut run = self.run(&args, CommandKind::Write).await?;
        let mut after = self.status().await?;

        // Rerere staged a recorded resolution for every conflicted path: keep
        // going. A round that makes no progress (the resolution left nothing
        // to commit) is a real stop and falls through to the conflict policy.
        for _ in 0..MAX_CONTINUES {
            let resolved = matches!(
                after.in_progress,
                Some(InProgress::Rebase | InProgress::RebaseApply)
            ) && after.worktree.conflicted == 0;
            if !resolved {
                break;
            }
            let stopped_at = after.oid().cloned();
            // git asks an editor to confirm the original commit's message;
            // this is the one call allowed to say yes to it unchanged.
            let next = command::run_keeping_messages(
                self.root(),
                &strings([
                    "-c",
                    "gc.auto=0",
                    "-c",
                    "rerere.enabled=true",
                    "-c",
                    "rerere.autoUpdate=true",
                    "rebase",
                    "--continue",
                ]),
                CommandKind::Write,
                &self.timeouts(),
            )
            .await?;
            let status = self.status().await?;
            let progressed = status.in_progress.is_none() || status.oid() != stopped_at.as_ref();
            tracing::debug!(progressed, code = ?next.code, "continued a rerere-resolved rebase");
            run = next;
            after = status;
            if !progressed {
                break;
            }
        }

        let outcome = classify_run(&RunReport {
            operation: Operation::Rebase,
            target: onto.as_str(),
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

    /// Push `new` to `refs/heads/<branch>` on `remote`, but only if the remote
    /// branch is still at `expected`.
    ///
    /// The one push rostrum makes; see [`crate::push`] for why it is shaped
    /// this way. A rejected lease is an `Ok(PushOutcome::Rejected(..))`: the
    /// remote is unchanged, and the caller decides what to tell the user.
    pub async fn push_with_lease(
        &self,
        remote: &Remote,
        branch: &BranchName,
        new: &Oid,
        expected: &Oid,
    ) -> Result<PushOutcome, GitError> {
        let run = self
            .run(
                &push_args(remote, branch, new, expected),
                CommandKind::Network,
            )
            .await?;
        let outcome = classify_push(&run.stdout, &run.stderr, run.success, run.code, branch)?;
        tracing::info!(
            branch = %branch,
            remote = %remote,
            new = %new.short(),
            expected = %expected.short(),
            ?outcome,
            "leased push"
        );
        Ok(outcome)
    }
}
