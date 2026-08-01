//! Domain types and pure state transformations for rostrum.
//!
//! No I/O and no `gpui` dependency, so everything here is testable with plain
//! `cargo test`.

pub mod feed;
pub mod model;
pub mod state;

pub use feed::flatten;
pub use feed::{Chrome, Feed, FeedFilter, FeedRow, PrIx, RepoIx};
pub use model::{
    CheckState, Label, Mergeable, PrNumber, PullRequest, RepoId, ReviewDecision, User,
};
pub use state::{AppState, LoadState, RepoState, Selection};
