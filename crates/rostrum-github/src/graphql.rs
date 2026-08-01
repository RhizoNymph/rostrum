//! GraphQL v4 queries and their wire types.
//!
//! One query per repository returns everything the feed needs, instead of
//! listing PRs and then fanning out a request per PR for reviews and checks.

use chrono::{DateTime, Utc};
use rostrum_core::{CheckState, Label, Mergeable, PrNumber, PullRequest, ReviewDecision, User};
use serde::Deserialize;

use crate::error::GraphQlError;

/// Open pull requests for one repository, most recently updated first.
pub const OPEN_PULL_REQUESTS: &str = r#"
query($owner: String!, $name: String!, $first: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number
        title
        url
        isDraft
        createdAt
        updatedAt
        author { login avatarUrl }
        headRefName
        headRefOid
        baseRefName
        additions
        deletions
        changedFiles
        mergeable
        reviewDecision
        labels(first: 10) { nodes { name color } }
        comments { totalCount }
        commits(last: 1) {
          nodes { commit { statusCheckRollup { state } } }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
pub struct GraphQlResponse<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Vec<GraphQlError>,
}

/// GraphQL connections may serialize `nodes` as `null`, and individual entries
/// may be `null` when the viewer lacks access, so both layers are optional.
#[derive(Debug, Deserialize)]
pub struct Connection<T> {
    // No `#[serde(default)]`: serde already treats a missing `Option` field as
    // `None`, and the attribute would add a spurious `T: Default` bound to the
    // generated `Deserialize` impl.
    pub nodes: Option<Vec<Option<T>>>,
}

// Hand-written so it does not pick up a spurious `T: Default` bound the way a
// derive would.
impl<T> Default for Connection<T> {
    fn default() -> Self {
        Self { nodes: None }
    }
}

impl<T> Connection<T> {
    pub fn into_vec(self) -> Vec<T> {
        self.nodes
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoQueryData {
    pub rate_limit: Option<RateLimit>,
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<RepositoryNode>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub cost: u32,
    pub remaining: u32,
    pub reset_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryNode {
    pub pull_requests: Connection<PrNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrNode {
    pub number: u32,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// `null` for deleted accounts and some bot actors.
    pub author: Option<AuthorNode>,
    pub head_ref_name: String,
    #[serde(default)]
    pub head_ref_oid: Option<String>,
    pub base_ref_name: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    #[serde(default = "unknown_mergeable")]
    pub mergeable: Mergeable,
    pub review_decision: Option<ReviewDecision>,
    pub labels: Option<Connection<LabelNode>>,
    pub comments: Option<TotalCount>,
    pub commits: Option<Connection<CommitEdge>>,
}

fn unknown_mergeable() -> Mergeable {
    Mergeable::Unknown
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorNode {
    pub login: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LabelNode {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalCount {
    pub total_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct CommitEdge {
    pub commit: CommitNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitNode {
    /// `null` when no CI is configured for the head commit.
    pub status_check_rollup: Option<StatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
pub struct StatusCheckRollup {
    pub state: Option<CheckState>,
}

impl PrNode {
    pub fn into_domain(self) -> PullRequest {
        let checks = self
            .commits
            .map(Connection::into_vec)
            .unwrap_or_default()
            .into_iter()
            .next()
            .and_then(|edge| edge.commit.status_check_rollup)
            .and_then(|rollup| rollup.state);

        PullRequest {
            number: PrNumber(self.number),
            title: self.title,
            url: self.url,
            is_draft: self.is_draft,
            created_at: self.created_at,
            updated_at: self.updated_at,
            author: self.author.map(|a| User {
                login: a.login,
                avatar_url: a.avatar_url,
            }),
            head_ref: self.head_ref_name,
            head_sha: self.head_ref_oid.unwrap_or_default(),
            base_ref: self.base_ref_name,
            additions: self.additions,
            deletions: self.deletions,
            changed_files: self.changed_files,
            mergeable: self.mergeable,
            review_decision: self.review_decision,
            labels: self
                .labels
                .map(Connection::into_vec)
                .unwrap_or_default()
                .into_iter()
                .map(|l| Label {
                    name: l.name,
                    color: l.color,
                })
                .collect(),
            comment_count: self.comments.map_or(0, |c| c.total_count),
            checks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like a real API response, including the fields most likely to be
    /// null in practice.
    const SAMPLE: &str = r#"{
      "data": {
        "rateLimit": { "cost": 1, "remaining": 4999, "resetAt": "2026-08-01T12:00:00Z" },
        "repository": {
          "pullRequests": {
            "nodes": [
              {
                "number": 42,
                "title": "Add the thing",
                "url": "https://github.com/a/b/pull/42",
                "isDraft": false,
                "createdAt": "2026-07-30T10:00:00Z",
                "updatedAt": "2026-07-31T11:00:00Z",
                "author": { "login": "octocat", "avatarUrl": "https://example.invalid/a.png" },
                "headRefName": "feature",
                "headRefOid": "deadbeefcafe",
                "baseRefName": "main",
                "additions": 10,
                "deletions": 2,
                "changedFiles": 3,
                "mergeable": "MERGEABLE",
                "reviewDecision": "APPROVED",
                "labels": { "nodes": [{ "name": "bug", "color": "d73a4a" }] },
                "comments": { "totalCount": 4 },
                "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }] }
              },
              {
                "number": 43,
                "title": "Draft work",
                "url": "https://github.com/a/b/pull/43",
                "isDraft": true,
                "createdAt": "2026-07-29T10:00:00Z",
                "updatedAt": "2026-07-29T10:00:00Z",
                "author": null,
                "headRefName": "wip",
                "baseRefName": "main",
                "additions": 0,
                "deletions": 0,
                "changedFiles": 0,
                "mergeable": "UNKNOWN",
                "reviewDecision": null,
                "labels": { "nodes": null },
                "comments": { "totalCount": 0 },
                "commits": { "nodes": [{ "commit": { "statusCheckRollup": null } }] }
              }
            ]
          }
        }
      }
    }"#;

    fn parse() -> Vec<PullRequest> {
        let response: GraphQlResponse<RepoQueryData> =
            serde_json::from_str(SAMPLE).expect("sample should decode");
        assert!(response.errors.is_empty());
        response
            .data
            .expect("data present")
            .repository
            .expect("repository present")
            .pull_requests
            .into_vec()
            .into_iter()
            .map(PrNode::into_domain)
            .collect()
    }

    #[test]
    fn decodes_a_fully_populated_pr() {
        let prs = parse();
        let pr = &prs[0];
        assert_eq!(pr.number, PrNumber(42));
        assert_eq!(pr.title, "Add the thing");
        assert_eq!(
            pr.author.as_ref().map(|a| a.login.as_str()),
            Some("octocat")
        );
        assert_eq!(pr.mergeable, Mergeable::Mergeable);
        assert_eq!(pr.review_decision, Some(ReviewDecision::Approved));
        assert_eq!(pr.checks, Some(CheckState::Success));
        assert_eq!(pr.head_sha, "deadbeefcafe");
        assert_eq!(pr.labels.len(), 1);
        assert_eq!(pr.comment_count, 4);
    }

    /// Null author, null label nodes, and absent CI must not fail the decode —
    /// all three occur routinely on real repositories.
    #[test]
    fn tolerates_nulls_across_optional_fields() {
        let prs = parse();
        let pr = &prs[1];
        assert!(pr.author.is_none());
        // A pull request whose head oid is absent must decode, not fail.
        assert!(pr.head_sha.is_empty());
        assert!(pr.labels.is_empty());
        assert!(pr.checks.is_none());
        assert!(pr.review_decision.is_none());
        assert_eq!(pr.mergeable, Mergeable::Unknown);
    }

    #[test]
    fn surfaces_errors_alongside_partial_data() {
        let body = r#"{
          "data": { "rateLimit": null, "repository": null },
          "errors": [{ "type": "NOT_FOUND", "message": "Could not resolve to a Repository" }]
        }"#;
        let response: GraphQlResponse<RepoQueryData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(response.errors.len(), 1);
        assert_eq!(response.errors[0].kind.as_deref(), Some("NOT_FOUND"));
        assert!(
            response
                .data
                .expect("data key present")
                .repository
                .is_none()
        );
    }

    #[test]
    fn decodes_rate_limit() {
        let response: GraphQlResponse<RepoQueryData> =
            serde_json::from_str(SAMPLE).expect("should decode");
        let limit = response
            .data
            .expect("data")
            .rate_limit
            .expect("rate limit present");
        assert_eq!(limit.remaining, 4999);
        assert_eq!(limit.cost, 1);
    }
}
