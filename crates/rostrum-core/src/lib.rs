//! Domain types and pure state transformations for rostrum.
//!
//! No I/O and no `gpui` dependency, so everything here is testable with plain
//! `cargo test`.

pub mod feed;
pub mod model;
pub mod state;
pub mod timeline;

pub use feed::flatten;
pub use feed::{Chrome, Feed, FeedFilter, FeedRow, PrIx, RepoIx};
pub use model::{
    CheckState, Divergence, Label, MergeStateStatus, MergeStatus, Mergeable, NodeId, PrNumber,
    PullRequest, Relation, RepoId, ReviewDecision, Side, User,
};
pub use state::{AppState, LoadState, RepoState, Selection, carry_forward_divergence};
pub use timeline::{
    CheckRun, CommentId, Conversation, EventKind, ReviewId, ReviewState, ReviewThread,
    ThreadComment, ThreadId, TimelineItem,
};
