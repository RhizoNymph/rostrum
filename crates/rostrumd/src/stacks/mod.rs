//! Stacks of pull requests, driven from a paired phone.
//!
//! The phone cannot run `gh`; the desktop can, on its clones. A stack request
//! goes:
//!
//! 1. **Clone.** The repository must have a configured clone (rostrum's
//!    `config.json`, read fresh); its lease is taken from the job coordinator
//!    — a busy clone is refused with 409 before anything else happens.
//! 2. **Snapshot.** The repository's open pull requests and GitHub's stacks
//!    are fetched now ([`snapshot`]), with the desktop's token.
//! 3. **Validate** ([`validate`]) with `rostrum-core`'s own `plan_stack` /
//!    `plan_extend`, and for a rewriting operation check that the request's
//!    `confirm_rewrite` names exactly the branches the plan rewrites.
//! 4. **Run** `rostrum-stack` ([`ops`]) in a task that holds the lease,
//!    recording progress and the final state ([`outcome`]) for polling.

pub mod ops;
pub mod outcome;
pub mod snapshot;
pub mod validate;

pub use ops::{GhStackOps, StackOps};
pub use snapshot::{GitHubSnapshots, RepoSnapshots, SnapshotError};
pub use validate::StackRequestError;

use std::path::PathBuf;

/// Where scratch worktrees for arranging go: the desktop app's own
/// directory, so a conflict handed off from the phone can be finished, and
/// the arrangement re-run, from either side.
pub fn default_scratch_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("rostrum")
        .join("stack-worktrees")
}
