//! The local half of a pull request, independent of any UI.
//!
//! Two questions and one verb: where is this branch checked out and how far
//! has it drifted ([`local_state`]), and run a pull/merge/rebase on it
//! ([`run_local_job`]). The desktop app and `rostrumd` both call these, so a
//! button on the desktop and a button on a paired phone cannot mean different
//! things.

mod jobs;
mod state;

pub use jobs::{LocalJob, LocalOp, LocalResult, run_local_job};
pub use state::{HandoffState, LocalBranch, LocalState, abort_in_progress, local_state};
