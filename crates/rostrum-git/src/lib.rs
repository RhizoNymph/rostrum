//! Local git operations, driven through the `git` command line.
//!
//! The crate exists so rostrum can answer "is this branch behind its base, and
//! what would it take to catch up" without leaving the review window, and act
//! on the answer.
//!
//! # The invariant everything else follows
//!
//! > `Err` means the repository is exactly as it was before the call.
//! > `Ok` means rostrum changed it and is reporting the new state.
//!
//! Three consequences shape the whole API:
//!
//! 1. **A conflict is not an error.** A rebase or merge that stops on a
//!    conflict did what it was asked; it arrives as [`Outcome::Conflicted`]
//!    carrying git's own message. Under the default [`ConflictPolicy::Abort`]
//!    the conflict has already been aborted, so the repository really is as it
//!    was. Under [`ConflictPolicy::Leave`] it is reported with the sequencer
//!    state still on disk, and [`Repo::conflict_context`] describes that state
//!    for whoever is going to finish it.
//! 2. **A refusal is not an error either.** A dirty worktree or a rebase
//!    already under way is a [`Blocker`], read from [`Repo::preflight`] before
//!    a button is drawn. It becomes [`GitError::Refused`] only for a caller
//!    that asked anyway.
//! 3. **[`GitError::Timeout`] is the single exception**, and carries
//!    [`GitError::may_have_written`] to say so.
//!
//! # How the answers are decided
//!
//! Not from exit codes. A rebase whose autostash pop conflicts exits *zero*
//! while leaving unmerged paths; a merge that conflicts exits one having done
//! precisely what was asked. So every write re-reads the repository afterwards
//! and [`classify_run`] derives the verdict from that state. The alternative —
//! matching git's English — is what `LC_ALL=C` and a single carefully scoped
//! prose match in [`fetch`] are there to keep to an absolute minimum.
//!
//! # Testing
//!
//! There are no mocks, here or anywhere in this workspace. Every decision is a
//! pure function over a string git actually printed or a file read from disk
//! — [`parse_status_v2`], [`parse_ab`], [`parse_left_right_count`],
//! [`in_progress`], [`classify_run`], [`classify_fetch`],
//! [`parse_fetch_porcelain`], [`blockers`], [`parse_worktree_list`],
//! [`parse_unmerged_v2`], [`extract_conflict_regions`], [`parse_log_z`],
//! [`BranchName::new`], [`Oid::parse`] — so the interesting cases are literals
//! in a test rather than a fixture repository on disk. The `examples/` run the
//! real thing against a real repository.

pub mod command;
pub mod context;
pub mod error;
pub mod fetch;
pub mod outcome;
pub mod preflight;
pub mod refs;
pub mod repo;
pub mod status;
pub mod worktree;

pub use command::{CommandKind, Timeouts};
pub use context::{
    Caps, Commands, CommitList, CommitSummary, ConflictBody, ConflictContext, ConflictKind,
    ConflictRegion, ConflictedPath, LOG_FORMAT, MarkerSides, RegionBudget, Side, StoppedOperation,
    body_from_file, extract_conflict_regions, parse_log_z, parse_rebase_progress,
    parse_unmerged_v2,
};
pub use error::GitError;
pub use fetch::{FetchFlag, FetchLine, FetchOutcome, classify_fetch, parse_fetch_porcelain};
pub use outcome::{AbortTarget, Conflict, Outcome, RunReport, classify_run};
pub use preflight::{Autostash, Blocker, Operation, Preflight, blockers};
pub use refs::{BranchName, NameRejection, Oid, Remote, RemoteRef, Rev};
pub use repo::{ConflictPolicy, Repo};
pub use status::{
    Head, InProgress, RepoStatus, StateFiles, Upstream, Worktree, in_progress, parse_ab,
    parse_left_right_count, parse_status_v2,
};
pub use worktree::{WorktreeEntry, parse_worktree_list};
