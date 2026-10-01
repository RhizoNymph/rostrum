//! Stacks of pull requests, made and merged through `gh stack`.
//!
//! The pure half — what a stack is, detecting chains, grouping, validating a
//! plan — lives in `rostrum-core`. This crate is the half that acts:
//!
//! - [`run_stack_job`] makes a [`StackPlan`](rostrum_core::StackPlan) into a
//!   stack from a clone: rebasing branches onto each other in scratch
//!   worktrees when they do not already chain, pushing each rewrite with a
//!   lease, then `gh stack link` and `gh stack init`.
//! - [`merge_stack`] and [`unstack`] run GitHub's atomic stack merge and its
//!   unstack.
//! - [`LocalStacks`] reads what `gh stack` keeps in the clone.
//!
//! Every `gh` call goes through one [`GhRunner`]; [`GhCli`] is the real one.
//! Nothing here depends on GPUI.

mod error;
pub mod gh;
mod job;
mod local_file;
mod remote;
mod run;

pub use error::StackOpError;
pub use gh::{GhCli, GhOutput, GhRunner, GhStackCommand, MergeMethod, StackView};
pub use job::{
    ExtendJob, LocalNote, LocalTracking, Progress, StackJob, StackOutcome, StackProgress,
    StackReport,
};
pub use local_file::{LocalBranch, LocalStack, LocalStacks, STACK_FILE};
pub use remote::{merge_stack, unstack};
pub use run::{run_extend_job, run_stack_job};
