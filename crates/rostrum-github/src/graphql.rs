//! GraphQL v4 queries and their wire types.
//!
//! One query per repository returns everything the feed needs, instead of
//! listing PRs and then fanning out a request per PR for reviews and checks.

use chrono::{DateTime, Utc};
use rostrum_core::{
    CheckState, Label, MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, ReviewDecision,
    User,
};
use serde::Deserialize;

use crate::error::GraphQlError;

/// Open pull requests for one repository, most recently updated first.
pub const OPEN_PULL_REQUESTS: &str = r#"
query($owner: String!, $name: String!, $first: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        id
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
        mergeStateStatus
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

/// Which side of the draft toggle a caller is asking for.
///
/// GitHub has no "set draft to X" mutation. The two directions are separate
/// operations with separate payload types, so the requested end state is what
/// selects the document — and modelling it as the end state rather than as
/// "toggle" keeps a stale view from flipping a pull request the wrong way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftState {
    Draft,
    ReadyForReview,
}

impl DraftState {
    /// The end state a pull request currently in `is_draft` should be moved to.
    pub fn toggled_from(is_draft: bool) -> Self {
        if is_draft {
            Self::ReadyForReview
        } else {
            Self::Draft
        }
    }

    /// What `PullRequest::is_draft` reads once this mutation lands.
    pub fn is_draft(self) -> bool {
        matches!(self, Self::Draft)
    }

    /// The mutation document that reaches this end state.
    pub fn mutation(self) -> &'static str {
        match self {
            Self::Draft => CONVERT_TO_DRAFT,
            Self::ReadyForReview => MARK_READY_FOR_REVIEW,
        }
    }

    /// Progressive label for the in-flight banner.
    pub fn progress_label(self) -> &'static str {
        match self {
            Self::Draft => "Converting to draft",
            Self::ReadyForReview => "Marking ready for review",
        }
    }
}

/// Both draft mutations alias their payload to `payload`, so one wire type
/// ([`SetDraftData`]) decodes either response.
pub const CONVERT_TO_DRAFT: &str = r#"
mutation($id: ID!) {
  payload: convertPullRequestToDraft(input: {pullRequestId: $id}) {
    pullRequest { id isDraft }
  }
}
"#;

pub const MARK_READY_FOR_REVIEW: &str = r#"
mutation($id: ID!) {
  payload: markPullRequestReadyForReview(input: {pullRequestId: $id}) {
    pullRequest { id isDraft }
  }
}
"#;

/// Response shape shared by both draft mutations, via the `payload` alias.
#[derive(Debug, Deserialize)]
pub struct SetDraftData {
    /// `null` when the mutation failed; the `errors` array carries the reason.
    pub payload: Option<SetDraftPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDraftPayload {
    /// `null` when the viewer may read the mutation result but not the pull
    /// request itself, which GitHub permits.
    pub pull_request: Option<DraftStateNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftStateNode {
    pub is_draft: bool,
}

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
    /// GraphQL node id. Carried on every pull request so the mutations that
    /// only exist in GraphQL never need a lookup round trip first.
    pub id: String,
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
    /// Both merge fields are defaulted rather than required. GitHub computes
    /// them lazily and has been observed to omit them entirely on the request
    /// that triggers the computation; a missing one must degrade to `Unknown`,
    /// not fail the whole repository's decode.
    #[serde(default)]
    pub mergeable: Mergeable,
    #[serde(default)]
    pub merge_state_status: MergeStateStatus,
    pub review_decision: Option<ReviewDecision>,
    pub labels: Option<Connection<LabelNode>>,
    pub comments: Option<TotalCount>,
    pub commits: Option<Connection<CommitEdge>>,
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
            node_id: NodeId(self.id),
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
            merge_state: self.merge_state_status,
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
                "id": "PR_kwDOAAAAAc4AAAAB",
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
                "mergeStateStatus": "BLOCKED",
                "reviewDecision": "APPROVED",
                "labels": { "nodes": [{ "name": "bug", "color": "d73a4a" }] },
                "comments": { "totalCount": 4 },
                "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }] }
              },
              {
                "id": "PR_kwDOAAAAAc4AAAAC",
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
        assert_eq!(pr.node_id, NodeId("PR_kwDOAAAAAc4AAAAB".into()));
        assert_eq!(pr.title, "Add the thing");
        assert_eq!(
            pr.author.as_ref().map(|a| a.login.as_str()),
            Some("octocat")
        );
        assert_eq!(pr.mergeable, Mergeable::Mergeable);
        assert_eq!(pr.merge_state, MergeStateStatus::Blocked);
        assert_eq!(pr.merge_status(), rostrum_core::MergeStatus::Blocked);
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
        // The second node omits `mergeStateStatus` altogether.
        assert_eq!(pr.merge_state, MergeStateStatus::Unknown);
    }

    // --- draft mutations ---------------------------------------------------

    /// The end state, not a toggle: a caller holding a stale `is_draft` asks
    /// for a specific side, and the two directions are distinct operations.
    #[test]
    fn draft_state_is_chosen_from_the_current_one() {
        assert_eq!(DraftState::toggled_from(true), DraftState::ReadyForReview);
        assert_eq!(DraftState::toggled_from(false), DraftState::Draft);
        assert!(DraftState::Draft.is_draft());
        assert!(!DraftState::ReadyForReview.is_draft());
    }

    /// Each direction must select its own mutation. Swapping these would
    /// silently do the opposite of what the button says.
    #[test]
    fn each_direction_selects_its_own_mutation() {
        assert!(
            DraftState::Draft
                .mutation()
                .contains("convertPullRequestToDraft")
        );
        assert!(
            DraftState::ReadyForReview
                .mutation()
                .contains("markPullRequestReadyForReview")
        );
    }

    /// Both documents must alias their payload, because one wire type decodes
    /// either response and the alias is what makes that work.
    #[test]
    fn both_mutations_alias_their_payload() {
        for query in [CONVERT_TO_DRAFT, MARK_READY_FOR_REVIEW] {
            assert!(query.contains("payload:"), "{query}");
            assert!(query.contains("isDraft"), "{query}");
        }
    }

    #[test]
    fn decodes_either_draft_mutation_response() {
        for (body, expected) in [
            (
                r#"{"data":{"payload":{"pullRequest":{"id":"PR_1","isDraft":true}}}}"#,
                true,
            ),
            (
                r#"{"data":{"payload":{"pullRequest":{"id":"PR_1","isDraft":false}}}}"#,
                false,
            ),
        ] {
            let response: GraphQlResponse<SetDraftData> =
                serde_json::from_str(body).expect("should decode");
            let is_draft = response
                .data
                .expect("data present")
                .payload
                .expect("payload present")
                .pull_request
                .expect("pull request present")
                .is_draft;
            assert_eq!(is_draft, expected);
        }
    }

    /// GitHub may apply the mutation and still withhold the pull request, so a
    /// null payload must decode rather than fail.
    #[test]
    fn tolerates_a_withheld_mutation_payload() {
        let body = r#"{"data":{"payload":{"pullRequest":null}}}"#;
        let response: GraphQlResponse<SetDraftData> =
            serde_json::from_str(body).expect("should decode");
        assert!(
            response
                .data
                .expect("data present")
                .payload
                .expect("payload present")
                .pull_request
                .is_none()
        );
    }

    /// A refused mutation answers HTTP 200 with a null payload and a populated
    /// `errors` array, which must not be mistaken for success.
    #[test]
    fn a_refused_mutation_carries_its_reason() {
        let body = r#"{
          "data": { "payload": null },
          "errors": [{ "message": "Pull request is already a draft" }]
        }"#;
        let response: GraphQlResponse<SetDraftData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(response.errors.len(), 1);
        assert!(response.data.expect("data key present").payload.is_none());
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
