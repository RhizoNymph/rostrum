//! The per-repository branch view's model: which branches are trunks, the
//! comparisons that measure them and the pull requests on them, and the tree
//! those form.
//!
//! The flow is `TrunkChoice` + [`RepoMeta`] → [`Trunks`] → [`ComparePlan`]
//! (one batched request) → [`BranchCounts`] → [`build_tree`] →
//! [`BranchTree::rows`]. Everything here is pure; the network half lives in
//! `rostrum-github` and the drawing in the desktop crate.

mod name;
mod plan;
mod rows;
mod tree;
mod trunks;

#[cfg(test)]
mod tests;

pub use name::{TrunkName, TrunkNameError};
pub use plan::{BranchCounts, CompareKey, ComparePlan, PlanError};
pub use rows::BranchRow;
pub use tree::{BaseGroup, BranchTree, PullNode, PullNote, TrunkDrift, TrunkNode, build_tree};
pub use trunks::{
    DEFAULT_TRUNK_CANDIDATES, OtherTrunk, RepoMeta, TrunkChoice, Trunks, default_candidates,
};
