//! The local half of a pull request, independent of any UI.
//!
//! Two questions and two verbs: where is this branch checked out and how far
//! has it drifted ([`local_state`]), run a pull/merge/rebase on it
//! ([`run_local_job`]), and what would pushing it do ([`plan_push`]) and do
//! it ([`run_push`]). The desktop app and `rostrumd` both call these, so a
//! button on the desktop and a button on a paired phone cannot mean different
//! things.

mod jobs;
mod push;
mod state;

pub use jobs::{LocalJob, LocalOp, LocalResult, run_local_job};
pub use push::{
    ConfirmedRewrite, PlannedEntry, PlannedPush, PushAllPlan, PushKind, PushPlan, PushResult,
    PushSkip, PushStep, PushTarget, Refresh, RewriteDecision, plan_push, plan_push_all,
    repo_of_url, run_push, url_names,
};
pub use state::{HandoffState, LocalBranch, LocalState, abort_in_progress, local_state};
