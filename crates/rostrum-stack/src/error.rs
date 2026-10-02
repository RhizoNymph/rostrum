//! The one error type of this crate.

use std::{path::PathBuf, time::Duration};

use rostrum_core::{PlanError, PrNumber};
use rostrum_git::GitError;

/// Everything that can stop a stack operation before it reaches a verdict.
///
/// A conflict, a rejected lease, or a failed `link` after pushes landed are
/// *not* here: they are outcomes the caller must render differently, and live
/// in [`crate::StackOutcome`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StackOpError {
    #[error(transparent)]
    Git(#[from] GitError),

    #[error(transparent)]
    Plan(#[from] PlanError),

    /// `gh` could not be started: not installed, or not on `PATH`.
    #[error("could not run `gh`; is the GitHub CLI installed?")]
    GhSpawn {
        #[source]
        source: std::io::Error,
    },

    /// `gh` was killed after its budget. For `merge` and `unstack` GitHub may
    /// still finish what was asked; the next refresh shows the truth.
    #[error("`{command}` timed out after {}s", after.as_secs())]
    GhTimeout { command: String, after: Duration },

    /// `gh` ran and declined, with its own explanation.
    #[error("`{command}` failed{}: {message}", code.map(|c| format!(" (exit {c})")).unwrap_or_default())]
    GhFailed {
        command: String,
        code: Option<i32>,
        message: String,
    },

    /// The `gh stack` extension is not installed.
    #[error(
        "the `gh stack` extension is not installed; run `gh extension install github/gh-stack`"
    )]
    GhStackMissing,

    #[error("could not decode `gh stack view --json`: {source}")]
    ViewDecode {
        #[source]
        source: serde_json::Error,
    },

    #[error("could not read `{}`: {source}", path.display())]
    LocalFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not decode `{}`: {source}", path.display())]
    LocalFileDecode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// A branch the plan names is not on the remote after fetching.
    #[error("`origin/{branch}` does not exist; was the branch of {number} deleted?")]
    MissingOnRemote { branch: String, number: PrNumber },

    #[error("the trunk `origin/{0}` does not exist")]
    MissingTrunk(String),

    /// A handoff session for one of the pull requests is already running;
    /// starting a second rewrite would race whatever it is doing.
    #[error("tmux session `{session}` is already handling {number}; finish or kill it first")]
    HandoffRunning { session: String, number: PrNumber },

    #[error("a conflict handler is configured but cannot be used: {0}")]
    Handler(String),

    #[error("could not prepare `{}`: {source}", path.display())]
    ScratchDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
