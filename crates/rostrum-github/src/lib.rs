//! GitHub data layer: token resolution, GraphQL reads, REST mutations.

pub mod auth;
pub mod client;
pub mod conversation;
pub mod error;
pub mod graphql;
pub mod rest;

pub use auth::{Token, resolve_token};
pub use client::GitHubClient;
pub use conversation::PULL_REQUEST_CONVERSATION;
pub use error::GitHubError;
pub use rest::{DraftComment, IssueState, MergeMethod, PullRequestFile, ReviewEvent, SubmitReview};
