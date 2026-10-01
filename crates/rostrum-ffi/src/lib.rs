//! The Rust core of rostrum's Android app, exposed to Kotlin through UniFFI.
//!
//! One object, [`RostrumCore`], owns everything: the settings file, the
//! SQLite cache and drafts, the GitHub session, and the paired desktop. Its
//! methods are grouped by the screen they serve, one module each:
//!
//! | Module | Serves |
//! |---|---|
//! | [`session`] | the GitHub token and who it belongs to |
//! | [`settings`] | repositories, cadence, notification toggles |
//! | [`feed`] | the multi-repository feed, its filter, the author roster |
//! | [`detail`] | a pull request's header, conversation, checks, actions |
//! | [`diff`] | the Files tab: overview and one file's diff |
//! | [`review`] | the pending review and its submission |
//! | [`remote`] | pairing with the desktop and its local worktree jobs |
//! | [`notifications`] | the background notification check |
//! | [`issues`] | the Issues tab's rows, the issue screen, issue actions, new issues |
//! | [`sort`] | the feed's repository and item sorts |
//! | [`stack_actions`] | making, arranging, extending, merging and unstacking stacks through the paired desktop |
//! | [`stacks`] | stacks of pull requests, grouped in the feed and on a repository |
//! | [`repo_view`] | one repository's screen and its branch tree |
//! | [`profiles`] | one profile per paired desktop or token, each its own core |
//!
//! Everything returned is render-ready: markdown is flattened into blocks,
//! diffs into highlighted rows with their comment anchors, chips carry text
//! and a colour role. Kotlin should not need to re-derive anything.
//!
//! See `docs/features/android_core.md`.

uniffi::setup_scaffolding!();

pub mod detail;
pub mod diff;
pub mod engine;
pub mod error;
pub mod feed;
pub mod issues;
pub mod logging;
pub mod markdown;
pub mod notifications;
pub mod profiles;
pub mod remote;
pub mod repo_view;
pub mod review;
pub mod session;
pub mod settings;
pub mod sort;
pub mod stack_actions;
pub mod stacks;
pub mod types;

#[cfg(test)]
pub(crate) mod test_support;

pub use engine::RostrumCore;
pub use error::{RemoteErrorCode, RostrumError};
pub use profiles::ProfileRegistry;
