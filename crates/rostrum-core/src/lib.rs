//! Domain types and pure state transformations for rostrum.
//!
//! No I/O and no `gpui` dependency, so everything here is testable with plain
//! `cargo test`.

pub mod arrivals;
pub mod authors;
pub mod feed;
pub mod model;
pub mod probe;
pub mod review;
pub mod state;
pub mod timeline;

#[cfg(test)]
pub(crate) mod test_support;

pub use arrivals::{Arrival, ArrivalKind, Baseline};
pub use authors::{AuthorEntry, VisibleAuthors, roster};
pub use feed::flatten;
pub use feed::{Chrome, Feed, FeedFilter, FeedRow, PrIx, RepoIx};
pub use model::{
    CheckState, Divergence, Label, LoginKey, MergeStateStatus, MergeStatus, Mergeable, NodeId,
    PrNumber, PullRequest, PullState, Relation, RepoId, ReviewDecision, Side, User,
};
pub use probe::{MergeProbeBudget, needs_merge_probe};
pub use review::{DraftAnchor, drafts_are_stale};
pub use state::{
    AppState, LoadState, RepoState, Selection, apply_divergences, carry_forward_divergence,
    divergence_query,
};
pub use timeline::{
    CheckRun, CommentId, Conversation, EventKind, ReviewId, ReviewState, ReviewThread,
    ThreadComment, ThreadId, TimelineItem,
};
