//! The single error type surfaced by this crate.
//!
//! The rule every variant upholds mirrors rostrum-git's: an `Err` from
//! [`hand_off`](crate::hand_off) before the spawn means nothing was started
//! and nothing on disk is half-written. The bundle is written atomically, so
//! a harness that later opens it either sees the whole file or a missing one,
//! never a prefix.

use std::{path::PathBuf, time::Duration};

/// Everything that can go wrong handing a stopped git operation to a tool.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HandoffError {
    /// The template never mentions `{context}`, so the handler would start
    /// with nothing to read. Caught before any git write, because a template
    /// this wrong is a configuration mistake and not something to discover
    /// with a rebase already stopped on conflicts.
    #[error("handoff command `{template}` does not mention `{{context}}`")]
    TemplateLacksContext { template: String },

    /// A path to substitute is not UTF-8 and cannot be quoted for a shell.
    /// The path would have to be typed into an interactive shell as text, and
    /// there is no lossless way to spell bytes that are not text.
    #[error("`{}` is not UTF-8 and cannot be passed to a shell", path.display())]
    PathNotUtf8 { path: PathBuf },

    /// No cache directory could be determined, so there is nowhere to put the
    /// bundle. `dirs::cache_dir` returns nothing when `$HOME` is unset and
    /// there is no `XDG_CACHE_HOME`, which is rare enough to be an error and
    /// not a fallback to the current directory.
    #[error("no cache directory could be determined")]
    NoCacheDir,

    /// The bundle could not be written. The caller aborts the git operation
    /// rather than leave it for a handler with nothing to read.
    #[error("could not write handoff bundle `{}`", path.display())]
    ContextWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// `tmux` is not installed or not on `PATH`. Nothing was spawned, and the
    /// bundle on disk is still there for the user to open by hand.
    #[error("could not run `tmux`")]
    TmuxMissing {
        #[source]
        source: std::io::Error,
    },

    /// tmux ran and refused; its stderr is the only useful thing to show. The
    /// arguments are carried so the user can reproduce the call.
    #[error("`tmux {args}` failed{}: {stderr}", code.map(|c| format!(" (exit {c})")).unwrap_or_default())]
    TmuxFailed {
        args: String,
        code: Option<i32>,
        stderr: String,
    },

    /// The tmux client did not answer in time. Only the client was killed; a
    /// harness, if any, is unaffected, because the client is a messenger to
    /// the server and the harness runs in a pane the server owns.
    #[error("`tmux` timed out after {}s", after.as_secs())]
    TmuxTimeout { after: Duration },
}
