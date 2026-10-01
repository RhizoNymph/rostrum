//! Stacks of pull requests: the model, detecting chains that could be one,
//! grouping them in the feed, and validating a request to make one. How a
//! stack sorts as one unit is `crate::sort::sort_key_for_group`.
//!
//! Pure, like the rest of this crate. Talking to `gh stack`, GitHub's Stacks
//! API, and git lives in `rostrum-stack`, `rostrum-github` and `rostrum-git`.

pub mod detect;
pub mod extend;
pub mod group;
pub mod model;
pub mod plan;

#[cfg(test)]
mod feed_tests;

pub use detect::detect_chains;
pub use extend::{Continuation, ExtendError, ExtendPlan, continuations, plan_extend};
pub use group::{FeedUnit, MergeRollup, StackGroup, StackIx, stack_groups, units};
pub use model::{RefName, Stack, StackError, StackMembers, StackNumber};
pub use plan::{PlanError, PlanMember, StackPlan, plan_stack};
