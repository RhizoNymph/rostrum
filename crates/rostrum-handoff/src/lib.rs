//! Handing a stopped git operation to a tool that can finish it.
//!
//! When a rebase or merge rostrum ran stops on conflicts, the repository is in
//! a state only a human — or a coding harness acting for one — can move
//! forward. This crate is the bridge: it writes everything a handler needs to
//! know into one Markdown file (the *bundle*), then starts the user's
//! configured command in a detached tmux session rooted in the worktree, with
//! the bundle's path substituted into the command line. The user attaches to
//! the session when they want to watch or take over.
//!
//! The pieces are independent and pure where they can be:
//!
//! - [`render_bundle`] turns a [`ConflictContext`] and pull request metadata
//!   into the Markdown text, most actionable section first.
//! - [`session_name`] and [`context_path`] derive one identity for the tmux
//!   session and the bundle file from the repository and PR number.
//! - [`substitute`] fills `{context}` and `{worktree}` into the configured
//!   template with shell quoting, and nothing else.
//! - [`spawn_argv`] is the exact tmux command line; [`spawn`] runs it.
//! - [`hand_off`] composes them in the order that keeps the invariant: the
//!   bundle is on disk, complete, before anything is started.
//!
//! # The environment, in one paragraph
//!
//! Unlike rostrum-git, which clears the environment because it parses git's
//! output and `GIT_DIR` or an askpass helper would change what git says, this
//! crate inherits everything and removes only `TMUX`. The tmux client parses
//! nothing; what runs in the end is the user's own interactive tool, and it
//! needs exactly what rostrum-git drops: `DISPLAY` for browser auth, the real
//! `TERM`, `ANTHROPIC_*`, the full `PATH`. And if this call starts the tmux
//! server, the server's global environment is a copy of ours, so a stripped
//! one would poison every session the user opens afterwards. The argument in
//! full is on [`session`].

pub mod bundle;
pub mod error;
pub mod session;
pub mod store;
pub mod template;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rostrum_git::ConflictContext;

pub use bundle::{DEFAULT_INSTRUCTIONS, Handoff, PrMeta, render_bundle};
pub use error::HandoffError;
pub use session::{Spawned, sanitise, session_exists, session_name, spawn, spawn_argv};
pub use store::{context_path, write_context};
pub use template::{shell_quote, substitute};

/// What [`hand_off`] did, for the UI to show and for a later click to find.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffReceipt {
    /// The tmux session name: `tmux attach -t =<session>`.
    pub session: String,
    /// Where the bundle was written.
    pub context_path: PathBuf,
    /// Whether this call started the session or found one already running.
    pub spawned: Spawned,
}

/// Write the bundle, then start the handler.
///
/// Order matters: the bundle is complete on disk before tmux is invoked, so
/// a handler can never start with nothing to read. An `Err` before the spawn
/// means nothing was started; the caller is expected to abort the git
/// operation in that case rather than leave a conflict nobody is handling.
///
/// Uses [`DEFAULT_INSTRUCTIONS`]; a caller with its own instructions renders
/// the bundle itself and calls the pieces.
pub async fn hand_off(
    pr: &PrMeta,
    context: &ConflictContext,
    worktree: &Path,
    command_template: &str,
    timeout: Duration,
) -> Result<HandoffReceipt, HandoffError> {
    let session = session_name(&pr.repo, pr.number);
    let path = context_path(&session)?;
    let text = render_bundle(&Handoff {
        pr,
        context,
        worktree,
        instructions: DEFAULT_INSTRUCTIONS,
    });
    write_context(&path, &text)?;
    let command = substitute(command_template, &path, worktree)?;
    let spawned = spawn(&session, worktree, &command, timeout).await?;
    Ok(HandoffReceipt {
        session,
        context_path: path,
        spawned,
    })
}
