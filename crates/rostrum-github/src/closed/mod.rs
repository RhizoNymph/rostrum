//! Closed pull requests: the read behind the feed's Closed tab.
//!
//! One GraphQL request per repository, like the open pull requests and the
//! issues, issued only while the Closed tab is in use; see
//! `docs/features/closed_prs.md` for the cadence.

pub mod wire;

#[cfg(test)]
mod tests;

use rostrum_core::{ClosedPull, RepoId};
use serde_json::json;

use crate::{client::GitHubClient, error::GitHubError, graphql::RateLimit};

pub use wire::{CLOSED_PULL_REQUESTS, ClosedNodeError};

/// How many closed pull requests one repository's fetch asks for: the 50
/// most recently updated. GitHub's page limit is 100; 50 covers weeks of a
/// busy repository at one rate-limit point per request.
pub const CLOSED_PER_REPO: u32 = 50;

/// Result of one repository's closed fetch.
#[derive(Debug)]
pub struct RepoClosedPulls {
    /// In fetch order (most recently updated first). The feed orders them
    /// by when they ended.
    pub closed: Vec<ClosedPull>,
    /// Nodes that could not be read as a merge or a close. Already logged;
    /// returned so a caller or test can see what was dropped.
    pub rejected: Vec<ClosedNodeError>,
    pub rate_limit: Option<RateLimit>,
}

impl GitHubClient {
    /// The `limit` most recently updated merged or closed pull requests of
    /// one repository.
    pub async fn closed_pull_requests(
        &self,
        repo: &RepoId,
        limit: u32,
    ) -> Result<RepoClosedPulls, GitHubError> {
        let resource = format!("{repo} closed pull requests");
        let data: wire::ClosedPullsData = self
            .graphql(
                CLOSED_PULL_REQUESTS,
                json!({ "owner": repo.owner(), "name": repo.name(), "first": limit }),
                &resource,
            )
            .await?;
        let rate_limit = data.rate_limit.clone();
        let decoded = data.into_domain().ok_or_else(|| GitHubError::NotFound {
            resource: repo.to_string(),
        })?;
        for rejected in &decoded.rejected {
            tracing::warn!(%repo, error = %rejected, "skipping a closed pull request");
        }
        Ok(RepoClosedPulls {
            closed: decoded.closed,
            rejected: decoded.rejected,
            rate_limit,
        })
    }
}
