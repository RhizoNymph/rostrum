//! Structured errors for the GitHub data layer.

use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("no GitHub token available: {0}")]
    NoToken(String),

    #[error("GitHub rejected the token; run `gh auth login` to refresh it")]
    Unauthorized,

    #[error("GitHub refused the request: {reason}")]
    Forbidden { reason: String },

    #[error("rate limit exhausted; resets at {reset_at}")]
    RateLimited { reset_at: DateTime<Utc> },

    #[error("secondary rate limit; retry after {} seconds", .retry_after_secs)]
    SecondaryRateLimit { retry_after_secs: u64 },

    #[error("{resource} not found (renamed, deleted, or not visible to this token)")]
    NotFound { resource: String },

    /// A GraphQL request can return HTTP 200 and still have failed. Partial
    /// data with a populated `errors` array is common when one repository in a
    /// batch is inaccessible, so this must be checked before trusting `data`.
    #[error("GraphQL error: {}", format_graphql_errors(.errors))]
    GraphQl { errors: Vec<GraphQlError> },

    #[error("GraphQL response contained no data")]
    EmptyData,

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("failed to decode {context}: {source}")]
    Decode {
        context: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("unexpected HTTP {status}: {body}")]
    Unexpected { status: u16, body: String },
}

impl GitHubError {
    /// Whether retrying later could plausibly succeed without user action.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Network(_)
                | Self::RateLimited { .. }
                | Self::SecondaryRateLimit { .. }
                | Self::Unexpected { .. }
        )
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct GraphQlError {
    pub message: String,
    #[serde(default)]
    pub path: Option<Vec<serde_json::Value>>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

fn format_graphql_errors(errors: &[GraphQlError]) -> String {
    errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}
