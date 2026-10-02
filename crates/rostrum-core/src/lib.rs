//! Domain types and pure state transformations for rostrum.
//!
//! No I/O and no `gpui` dependency, so everything here is testable with plain
//! `cargo test`.

pub mod arrivals;
pub mod authors;
pub mod branches;
pub mod feed;
pub mod issue;
pub mod model;
pub mod navigation;
pub mod probe;
pub mod repo_meta;
pub mod review;
pub mod sort;
pub mod stack;
pub mod state;
pub mod tabs;
pub mod timeline;

#[cfg(test)]
pub(crate) mod test_support;

pub use arrivals::{Arrival, ArrivalKind, Baseline};
pub use authors::{AuthorEntry, VisibleAuthors, issue_roster, roster};
pub use feed::{
    Chrome, Feed, FeedFilter, FeedRow, FeedStack, IssueIx, PrIx, RepoIx, StackPlace, StackSlot,
};
pub use feed::{flatten, flatten_in, flatten_tab, flatten_tab_in, repo_pull_rows};
pub use issue::{
    CloseReason, EmptyTitle, Issue, IssueDetail, IssueNumber, IssueState, IssueTitle, Milestone,
};
pub use model::{
    CheckState, Divergence, Label, LoginKey, MergeStateStatus, MergeStatus, Mergeable, NodeId,
    PrNumber, PullRequest, PullState, Relation, RepoId, ReviewDecision, Side, User,
};
pub use navigation::{RepoScreen, Screen};
pub use probe::{MergeProbeBudget, needs_merge_probe};
pub use repo_meta::{OwnerKind, RepoMeta, RepoOwner};
pub use review::{DraftAnchor, drafts_are_stale};
pub use sort::{
    FeedOrder, FeedSort, ItemSortKey, KeyKind, RepoSortKey, Sort, SortDirection, SortKey,
};
pub use stack::{
    FeedUnit, MergeRollup, PlanError, PlanMember, RefName, Stack, StackError, StackGroup, StackIx,
    StackMembers, StackNumber, StackPlan, plan_stack,
};
pub use state::{
    AppState, LoadState, RepoState, Selection, apply_divergences, carry_forward_divergence,
    divergence_query,
};
pub use tabs::{FeedTab, TabCounts, tab_counts};
pub use timeline::{
    CheckRun, CommentId, Conversation, EventKind, ReviewId, ReviewState, ReviewThread,
    ThreadComment, ThreadId, TimelineItem,
};
