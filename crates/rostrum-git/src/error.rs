//! The single error type surfaced by this crate.
//!
//! Note what is deliberately *absent*, because it is most of what a git
//! front-end has to deal with.
//!
//! A merge or rebase that stops on a conflict is **not** an error. git did
//! exactly what it was asked, the repository is in a state this crate can
//! describe, and the caller needs those details rather than a failure string.
//! It arrives as [`Outcome::Conflicted`](crate::Outcome::Conflicted).
//!
//! A pre-flight refusal — a dirty worktree, a rebase already under way, a
//! detached HEAD — is **not** an error either. It is a [`Blocker`] the caller
//! reads from [`Repo::preflight`](crate::Repo::preflight) so a button can be
//! greyed out with a reason before anything is attempted. Only a caller that
//! read the blockers and proceeded anyway gets [`GitError::Refused`].
//!
//! The rule every variant below upholds:
//!
//! > `Err` means the repository is exactly as it was before the call.
//! > `Ok` means rostrum changed it and is reporting the new state.
//!
//! [`GitError::Timeout`] is the one exception, and carries
//! [`GitError::may_have_written`] so a caller never has to guess which it is.

use std::{path::PathBuf, time::Duration};

use crate::{
    command::CommandKind,
    preflight::{Blocker, Operation},
    refs::NameRejection,
    status::InProgress,
};

/// Everything that can go wrong driving the `git` command line.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GitError {
    /// The path is not inside a work tree. Separate from [`GitError::Bare`]
    /// because the fix differs: this one needs a different path, that one needs
    /// a different repository.
    #[error("`{}` is not a git work tree", path.display())]
    NotARepository { path: PathBuf, stderr: String },

    /// A bare repository has no worktree, so nothing this crate offers — status,
    /// merge, rebase — has a meaning there. Rejected once at
    /// [`Repo::open`](crate::Repo::open) rather than failing per call.
    #[error("`{}` is a bare repository", path.display())]
    Bare { path: PathBuf },

    /// `git` could not be started at all: not installed, not on `PATH`, or not
    /// executable. Nothing ran, so nothing changed.
    #[error("could not run `git`")]
    Spawn {
        #[source]
        source: std::io::Error,
    },

    /// A filesystem probe failed for a reason other than "not there". The
    /// per-worktree sequencer files are checked with [`std::fs::metadata`]; a
    /// missing file is the normal answer, but a permission or I/O failure must
    /// not be silently read as "no rebase in progress".
    #[error("could not inspect `{}`", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The child was killed after exceeding its budget. **The only variant that
    /// may leave the repository changed:** a rebase killed mid-flight can leave
    /// sequencer state behind, and a killed `merge` can leave `MERGE_HEAD`.
    /// Read [`GitError::may_have_written`] rather than assuming either way; a
    /// timed-out read (`status`, `rev-list`) never writes.
    #[error("`git {command}` timed out after {}s", after.as_secs())]
    Timeout {
        command: String,
        after: Duration,
        kind: CommandKind,
    },

    /// git ran to completion and declined. This covers every refusal that is
    /// not a conflict and not a pre-flight blocker — an unknown remote, a
    /// protected ref, a failed hook. git's own stderr is carried verbatim
    /// because it is nearly always the most useful thing to show a user.
    #[error("`git {command}` failed{}: {stderr}", code.map(|c| format!(" (exit {c})")).unwrap_or_default())]
    Failed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },

    /// The caller read a [`Blocker`] from [`Repo::preflight`](crate::Repo::preflight)
    /// — or skipped the call — and asked for the operation anyway. Refusing here
    /// is what keeps the invariant true: rostrum did not touch the repository.
    #[error("{operation} refused: {}", blocker.reason())]
    Refused {
        operation: Operation,
        blocker: Blocker,
    },

    /// git's output did not have the shape this crate parses. Every invocation
    /// pins its own format (`--porcelain=v2`, `--porcelain`, `-z`), so this
    /// means a genuine surprise rather than locale or configuration drift.
    #[error("could not parse {what}: `{line}`")]
    Parse { what: &'static str, line: String },

    /// A branch name was rejected before it could reach a command line. A
    /// branch really can be named `--upload-pack=...`, and git would read it as
    /// an option, so names are validated at construction rather than escaped at
    /// use.
    #[error("`{name}` is not a usable branch name: {}", reason.describe())]
    InvalidBranchName { name: String, reason: NameRejection },

    /// An object id was not the 40 or 64 lowercase hex digits git prints.
    #[error("`{value}` is not an object id")]
    InvalidOid { value: String },

    /// [`Repo::conflict_context`](crate::Repo::conflict_context) was asked to
    /// describe a stopped rebase or merge, but the worktree is not in one this
    /// crate started. `None` means nothing is in progress at all; `Some` is a
    /// foreign operation — `am`, cherry-pick, revert, bisect — which is the
    /// user's own and not rostrum's to describe or continue.
    #[error("nothing to describe: {}", in_progress.map(InProgress::describe).unwrap_or("no operation is in progress"))]
    NothingToDescribe { in_progress: Option<InProgress> },
}

impl GitError {
    /// Whether the repository may have been modified despite the `Err`.
    ///
    /// True for exactly one case: a write command killed by its timeout. Every
    /// other variant guarantees the repository is as it was.
    pub fn may_have_written(&self) -> bool {
        matches!(
            self,
            Self::Timeout {
                kind: CommandKind::Write,
                ..
            }
        )
    }

    /// Whether retrying later could plausibly succeed without user action.
    ///
    /// Note this is not "safe to retry": a [`GitError::Timeout`] from a write
    /// is transient *and* may have left state behind, so a caller should check
    /// [`GitError::may_have_written`] before repeating the call.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::Io { .. })
    }
}
