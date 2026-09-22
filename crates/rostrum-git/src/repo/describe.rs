//! [`Repo::conflict_context`]: the I/O behind [`crate::context`].
//!
//! A child module of [`crate::repo`] so it can reach the resolved per-worktree
//! [`StatePaths`](super::StatePaths) without widening their visibility. Every
//! decision made on what is read here is a pure function in
//! [`crate::context`]; this file only spawns, reads, and assembles.

use std::path::Path;

use crate::{
    command::CommandKind,
    context::{
        Caps, CommitList, CommitSummary, ConflictBody, ConflictContext, ConflictedPath, LOG_FORMAT,
        RegionBudget, StoppedOperation, body_from_file, parse_log_z, parse_rebase_progress,
        parse_unmerged_v2,
    },
    error::GitError,
    refs::{BranchName, Oid, RemoteRef, Rev},
    status::{InProgress, in_progress, parse_status_v2},
};

use super::{Repo, read_optional, status_args, strings};

impl Repo {
    /// Describe the rebase or merge currently stopped in this work tree.
    ///
    /// Read from the repository, not from a [`Conflict`] in hand, so the same
    /// call serves "it just stopped" and "it was found stopped on a later
    /// load". `branch` is the branch being rebased or merged into and `target`
    /// the ref it was pointed at; both are the caller's to know, because
    /// nothing on disk records the tracking ref by name.
    ///
    /// Four spawns, each bounded: one `status` (the same arguments as
    /// [`Repo::status`], so the unmerged paths and HEAD describe one instant),
    /// one `rev-list` for the true counts, and one `log` per side. Plus, for
    /// a rebase, one `show` of `REBASE_HEAD`. The conflicted files themselves
    /// are read from the working tree, never through `git diff` — see
    /// [`crate::context`] for why.
    pub async fn conflict_context(
        &self,
        branch: &BranchName,
        target: &RemoteRef,
    ) -> Result<ConflictContext, GitError> {
        self.conflict_context_with(branch, target, Caps::default())
            .await
    }

    /// [`Repo::conflict_context`] with explicit caps.
    pub async fn conflict_context_with(
        &self,
        branch: &BranchName,
        target: &RemoteRef,
        caps: Caps,
    ) -> Result<ConflictContext, GitError> {
        let run = self
            .run(&status_args(), CommandKind::Read)
            .await?
            .require_success("status")?;
        let (head, _) = parse_status_v2(&run.stdout)?;
        let unmerged = parse_unmerged_v2(&run.stdout)?;

        let state = &self.inner.state;
        let operation = match in_progress(state.probe()?) {
            Some(InProgress::Rebase) => {
                self.stopped_rebase(&state.rebase_merge, "msgnum", "end")
                    .await?
            }
            Some(InProgress::RebaseApply) => {
                self.stopped_rebase(&state.rebase_apply, "next", "last")
                    .await?
            }
            Some(InProgress::Merge) => StoppedOperation::Merge {
                merging: read_oid(&state.merge_head)?,
            },
            other => return Err(GitError::NothingToDescribe { in_progress: other }),
        };

        // A stopped operation always has a HEAD to have stopped at; an unborn
        // one here is a shape this crate cannot have produced.
        let head = head.oid().cloned().ok_or_else(|| GitError::Parse {
            what: "a stopped operation on an unborn HEAD",
            line: String::new(),
        })?;

        let mut budget = RegionBudget::default();
        let paths = unmerged
            .into_iter()
            .map(|(kind, path)| {
                let body = if kind.has_marked_file() {
                    match std::fs::read(self.inner.root.join(&path)) {
                        Ok(bytes) => body_from_file(&bytes, &caps, &mut budget),
                        Err(source) => ConflictBody::Absent {
                            reason: format!("could not read the working-tree file: {source}"),
                        },
                    }
                } else {
                    ConflictBody::Absent {
                        reason: format!("{}: git wrote no merged copy to show", kind.describe()),
                    }
                };
                ConflictedPath { path, kind, body }
            })
            .collect();

        let target_ref = target.tracking_ref();
        let branch_ref = branch.qualified();
        let divergence = self
            .divergence(&Rev::Local(branch.clone()), &Rev::Remote(target.clone()))
            .await?;
        let branch_commits = CommitList {
            commits: self
                .log_range(&target_ref, &branch_ref, caps.max_commits)
                .await?,
            total: divergence.ahead,
        };
        let target_commits = CommitList {
            commits: self
                .log_range(&branch_ref, &target_ref, caps.max_commits)
                .await?,
            total: divergence.behind,
        };

        Ok(ConflictContext {
            operation,
            branch: branch.clone(),
            target: target_ref,
            head,
            paths,
            branch_commits,
            target_commits,
            git_message: String::new(),
            caps,
        })
    }

    /// The rebase-specific half of [`Repo::conflict_context`]. `dir` is the
    /// per-worktree sequencer directory and the two names are its progress
    /// files, which differ between the merge and apply backends.
    async fn stopped_rebase(
        &self,
        dir: &Path,
        current: &str,
        total: &str,
    ) -> Result<StoppedOperation, GitError> {
        let step = match (
            read_optional(&dir.join(current))?,
            read_optional(&dir.join(total))?,
        ) {
            (Some(current), Some(total)) => parse_rebase_progress(&current, &total),
            _ => None,
        };
        let onto = read_optional(&dir.join("onto"))?
            .map(|contents| Oid::parse(contents.trim()))
            .transpose()?;
        Ok(StoppedOperation::Rebase {
            step,
            applying: self.rebase_head().await?,
            onto,
        })
    }

    /// The commit a rebase stopped on, or `None` when it stopped somewhere
    /// else.
    ///
    /// `REBASE_HEAD` is absent when the stop was not on a commit — a failed
    /// `--exec`, for one — so a non-zero exit is an answer, not a failure.
    async fn rebase_head(&self) -> Result<Option<CommitSummary>, GitError> {
        let format = format!("--format={LOG_FORMAT}");
        let run = self
            .run(
                &strings([
                    "show",
                    "-s",
                    format.as_str(),
                    "--end-of-options",
                    "REBASE_HEAD",
                    "--",
                ]),
                CommandKind::Read,
            )
            .await?;
        if !run.success {
            return Ok(None);
        }
        Ok(parse_log_z(&run.stdout)?.into_iter().next())
    }

    /// The commits in `from..to`, newest first, at most `max`.
    async fn log_range(
        &self,
        from: &str,
        to: &str,
        max: u32,
    ) -> Result<Vec<CommitSummary>, GitError> {
        let run = self
            .run(
                &strings([
                    "log",
                    "-z",
                    &format!("--max-count={max}"),
                    "--no-decorate",
                    &format!("--format={LOG_FORMAT}"),
                    "--end-of-options",
                    &format!("{from}..{to}"),
                    "--",
                ]),
                CommandKind::Read,
            )
            .await?
            .require_success("log")?;
        parse_log_z(&run.stdout)
    }
}

/// `MERGE_HEAD` and friends hold one object id per line. Only the first is
/// wanted: rostrum never starts an octopus merge.
fn read_oid(path: &Path) -> Result<Oid, GitError> {
    let contents = read_optional(path)?.ok_or_else(|| GitError::Parse {
        what: "a sequencer file that was present a moment ago",
        line: path.display().to_string(),
    })?;
    Oid::parse(contents.lines().next().unwrap_or("").trim())
}
