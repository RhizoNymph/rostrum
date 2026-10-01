//! Stacks of pull requests: the model, detecting chains that could be one,
//! grouping them in the feed, sorting a stack as one unit, and validating a
//! request to make one.
//!
//! Pure, like the rest of this crate. Talking to `gh stack`, GitHub's Stacks
//! API, and git lives in `rostrum-stack`, `rostrum-github` and `rostrum-git`.

pub mod detect;
pub mod group;
pub mod model;
pub mod plan;
pub mod sort;

#[cfg(test)]
mod feed_tests;

pub use detect::detect_chains;
pub use group::{FeedUnit, MergeRollup, StackGroup, StackIx, stack_groups, units};
pub use model::{RefName, Stack, StackError, StackMembers, StackNumber};
pub use plan::{PlanError, PlanMember, StackPlan, plan_stack};
pub use sort::{KeyKind, SortDirection, compare_units, default_order, unit_key};
