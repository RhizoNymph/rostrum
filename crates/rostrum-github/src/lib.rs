//! GitHub data layer: token resolution, GraphQL reads, REST mutations.

pub mod auth;
pub mod client;
pub mod error;
pub mod graphql;

pub use auth::{Token, resolve_token};
pub use client::GitHubClient;
pub use error::GitHubError;
