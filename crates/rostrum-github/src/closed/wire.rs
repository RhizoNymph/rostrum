//! The closed pull request document and the wire types it decodes into.

use chrono::{DateTime, Utc};
use rostrum_core::{ClosedPull, EndingError, PullEnding, PullState};
use serde::Deserialize;

use crate::{
    conversation::lenient_state,
    graphql::{Connection, PrNode, RateLimit},
};

/// One repository's most recently *updated* merged or closed pull requests.
///
/// GitHub's `PullRequestOrderField` offers `CREATED_AT` and `UPDATED_AT`
/// only; nothing orders by `mergedAt` or `closedAt`. Update order is the
/// closest proxy — ending a pull request updates it — so this window holds
/// every recent ending, plus older pull requests touched since (a late
/// comment, a label), which push the oldest endings out of the window. The
/// Closed tab sorts what arrives by when each ended.
///
/// The selection is the feed's summary fields minus what only an open pull
/// request has a use for — mergeability, the check rollup, the push time,
/// review requests' detail — plus `state`, `mergedAt` and `closedAt`, which
/// decide the ending. Two bounded connections of ten per node, like the
/// open query; GitHub costs it at one point.
pub const CLOSED_PULL_REQUESTS: &str = r#"
query($owner: String!, $name: String!, $first: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequests(states: [MERGED, CLOSED], first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        id
        number
        title
        url
        state
        isDraft
        createdAt
        updatedAt
        closedAt
        mergedAt
        author { login avatarUrl }
        headRefName
        headRefOid
        baseRefName
        isCrossRepository
        additions
        deletions
        changedFiles
        reviewDecision
        assignees(first: 10) { nodes { login avatarUrl } }
        labels(first: 10) { nodes { name color } }
        comments { totalCount }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedPullsData {
    pub rate_limit: Option<RateLimit>,
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<ClosedRepositoryNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedRepositoryNode {
    pub pull_requests: Option<Connection<ClosedNode>>,
}

/// One node: the fields every pull request query shares, decoded by the
/// same [`PrNode`], plus how it ended.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedNode {
    #[serde(flatten)]
    pub pull: PrNode,
    /// `OPEN`, `CLOSED` or `MERGED`; a value this build does not know
    /// decodes as `None`, which rejects the node rather than the list.
    #[serde(default, deserialize_with = "lenient_state")]
    pub state: Option<PullState>,
    pub merged_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
}

/// Why one node of a closed list could not become a [`ClosedPull`].
///
/// Per node, not per list: one odd pull request is dropped and logged, and
/// the rest of the repository's list still shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ClosedNodeError {
    #[error("pull request #{number} has a state this build does not know")]
    UnknownState { number: u32 },
    #[error("pull request #{number}: {source}")]
    Ending {
        number: u32,
        #[source]
        source: EndingError,
    },
}

impl ClosedNode {
    pub fn into_domain(self) -> Result<ClosedPull, ClosedNodeError> {
        let number = self.pull.number;
        let state = self.state.ok_or(ClosedNodeError::UnknownState { number })?;
        let ending = PullEnding::from_parts(state, self.merged_at, self.closed_at)
            .map_err(|source| ClosedNodeError::Ending { number, source })?;
        Ok(ClosedPull {
            pull: self.pull.into_domain(),
            ending,
        })
    }
}

/// A decoded list: what became closed pull requests, in fetch order, and
/// what was rejected.
#[derive(Debug, Default)]
pub struct DecodedClosed {
    pub closed: Vec<ClosedPull>,
    pub rejected: Vec<ClosedNodeError>,
}

impl ClosedPullsData {
    /// The repository's closed list, or `None` when the repository itself
    /// was withheld.
    pub fn into_domain(self) -> Option<DecodedClosed> {
        let repository = self.repository?;
        let mut decoded = DecodedClosed::default();
        for node in repository
            .pull_requests
            .map(Connection::into_vec)
            .unwrap_or_default()
        {
            match node.into_domain() {
                Ok(closed) => decoded.closed.push(closed),
                Err(rejected) => decoded.rejected.push(rejected),
            }
        }
        Some(decoded)
    }
}
