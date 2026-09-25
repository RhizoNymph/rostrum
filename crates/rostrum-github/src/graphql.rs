//! GraphQL v4 queries and their wire types.
//!
//! One query per repository returns everything the feed needs, instead of
//! listing PRs and then fanning out a request per PR for reviews and checks.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rostrum_core::{
    CheckState, Divergence, Label, MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest,
    ReviewDecision, User,
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

// --- branch divergence ---------------------------------------------------

/// How far a pull request's head branch has drifted from its base, in one
/// round trip.
///
/// `Ref.compare` answers with both counts at once, which is what the caller
/// needs: "behind by 3" alone cannot tell a branch that will fast-forward from
/// one that has also moved on, and those two want different buttons.
///
/// `status` is selected but deliberately absent from the wire types below. It
/// is GitHub's own one-word summary of the very same two counts, and
/// [`Divergence::relation`] derives that verdict already; keeping it in the
/// document makes a captured response self-explanatory, while leaving it out of
/// the wire types means a future addition to `ComparisonStatus` cannot fail
/// this query's decode.
pub const PULL_REQUEST_DIVERGENCE: &str = r#"
query($owner: String!, $name: String!, $base: String!, $head: String!) {
  repository(owner: $owner, name: $name) {
    ref(qualifiedName: $base) {
      compare(headRef: $head) { aheadBy behindBy status }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
pub struct DivergenceQueryData {
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<DivergenceRepositoryNode>,
}

#[derive(Debug, Deserialize)]
pub struct DivergenceRepositoryNode {
    /// `ref` is a Rust keyword, so the field is renamed rather than raw-named.
    /// `null` when the base branch itself cannot be resolved — a base that was
    /// deleted or renamed out from under an open pull request.
    #[serde(rename = "ref")]
    pub base_ref: Option<BaseRefNode>,
}

#[derive(Debug, Deserialize)]
pub struct BaseRefNode {
    /// `null` — alongside a `NOT_FOUND` entry in `errors` — whenever the base
    /// repository cannot resolve the head ref, which is the ordinary answer for
    /// a pull request from a fork. That is the signal to compare locally
    /// instead, so it must decode rather than fail.
    pub compare: Option<ComparisonNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonNode {
    pub ahead_by: u32,
    pub behind_by: u32,
}

impl ComparisonNode {
    /// GitHub counts both sides relative to the *head* ref, which is the
    /// direction [`Divergence`] fixes: `behind` is work the pull request has
    /// not caught up with. Mapping these the other way round would offer to
    /// update a branch that is already current.
    pub fn into_domain(self) -> Divergence {
        Divergence::new(self.ahead_by, self.behind_by)
    }
}

impl DivergenceQueryData {
    /// `None` at whichever level GitHub declined to resolve something, so one
    /// absent answer looks the same to the caller however deep it occurred and
    /// falls back to a local comparison rather than surfacing a failure.
    pub fn into_domain(self) -> Option<Divergence> {
        self.repository
            .and_then(|repository| repository.base_ref)
            .and_then(|base_ref| base_ref.compare)
            .map(ComparisonNode::into_domain)
    }
}

// --- batched divergence --------------------------------------------------

/// The document for [`build_divergence_batch`]: one `ref { compare }`
/// selection per pull request, aliased `p0..pN`, each bound to its own
/// `$bN`/`$hN` variable pair.
///
/// Aliasing is what makes this one request rather than N. GitHub costs the
/// document as a single query, and the feed refresh that triggers it is
/// already one request per repository, so the follow-up must not be N more.
///
/// Branch names travel as variables, never spliced into the document text: a
/// name with a quote, a brace or a `#` in it would otherwise change the
/// document's shape, and a variable is the one place GraphQL guarantees a
/// string stays a string. The document itself is built from `count` alone.
///
/// Only meaningful for `count >= 1`. An empty selection set is a syntax error
/// in GraphQL, so the client short-circuits an empty batch before reaching
/// here rather than sending a document GitHub would reject.
pub fn build_divergence_batch(count: usize) -> String {
    let mut declarations = String::from("$owner: String!, $name: String!");
    let mut selections = String::new();
    for index in 0..count {
        declarations.push_str(&format!(", $b{index}: String!, $h{index}: String!"));
        selections.push_str(&format!(
            "    {}: ref(qualifiedName: $b{index}) {{ compare(headRef: $h{index}) {{ aheadBy behindBy }} }}\n",
            divergence_alias(index)
        ));
    }
    format!(
        "query({declarations}) {{\n  repository(owner: $owner, name: $name) {{\n{selections}  }}\n}}\n"
    )
}

/// The variables a [`build_divergence_batch`] document of the same length
/// expects, with `pairs[i]` bound to `$b{i}`/`$h{i}`.
///
/// Built here beside the document so the two cannot drift: the test that
/// checks one against the other is the contract.
pub fn divergence_batch_variables(
    owner: &str,
    name: &str,
    pairs: &[(String, String)],
) -> serde_json::Value {
    let mut variables = serde_json::Map::new();
    variables.insert("owner".into(), owner.into());
    variables.insert("name".into(), name.into());
    for (index, (base, head)) in pairs.iter().enumerate() {
        variables.insert(format!("b{index}"), base.as_str().into());
        variables.insert(format!("h{index}"), head.as_str().into());
    }
    serde_json::Value::Object(variables)
}

/// The alias under which pair `index` is selected.
fn divergence_alias(index: usize) -> String {
    format!("p{index}")
}

/// Response to a [`build_divergence_batch`] document.
///
/// The aliases are dynamic — `p0..pN` for however many pairs were sent — so
/// no struct could name them. `repository` decodes as a map from alias to the
/// same [`BaseRefNode`] the single query uses, and [`Self::into_domain`]
/// walks it in the order the pairs were sent so the result stays
/// index-aligned with the request.
#[derive(Debug, Deserialize)]
pub struct DivergenceBatchData {
    /// `null` when the repository does not exist or is not visible. Each value
    /// is `null` when that base ref cannot be resolved, exactly as in
    /// [`DivergenceRepositoryNode::base_ref`].
    pub repository: Option<HashMap<String, Option<BaseRefNode>>>,
}

impl DivergenceBatchData {
    /// One entry per pair sent, `None` wherever GitHub declined at any level
    /// — the repository, that pair's base ref, or the comparison itself — so
    /// the caller sees one shape of "unknown" however deep the null was.
    ///
    /// An alias missing from the map altogether is treated the same way
    /// rather than as a decode failure: the request asked for it, so its
    /// absence is GitHub withholding an answer, not a malformed response.
    pub fn into_domain(self, count: usize) -> Vec<Option<Divergence>> {
        let mut repository = self.repository.unwrap_or_default();
        (0..count)
            .map(|index| {
                repository
                    .remove(&divergence_alias(index))
                    .flatten()
                    .and_then(|base_ref| base_ref.compare)
                    .map(ComparisonNode::into_domain)
            })
            .collect()
    }
}

/// Split a batch response's `errors` into the ones the batch tolerates and
/// the ones that fail it.
///
/// A `NOT_FOUND` whose path is `["repository", "pN", ...]` is the cross-fork
/// answer for pair N alone: the base repository cannot resolve a head ref
/// that lives in a fork, and GitHub reports that per alias while still
/// answering every other alias. Those are expected and are dropped; the
/// matching `compare` is already `null` in `data`, which is where the caller
/// reads the `None` from.
///
/// Everything else is returned for the caller to fail on: a `NOT_FOUND` for
/// the repository itself (path `["repository"]`, or no path at all), which
/// means no alias was answered, and any error of another kind, which the
/// batch has no fallback for.
pub fn unexcused_batch_errors(errors: Vec<GraphQlError>, count: usize) -> Vec<GraphQlError> {
    errors
        .into_iter()
        .filter(|error| !is_alias_not_found(error, count))
        .collect()
}

fn is_alias_not_found(error: &GraphQlError, count: usize) -> bool {
    if error.kind.as_deref() != Some("NOT_FOUND") {
        return false;
    }
    let Some(path) = error.path.as_deref() else {
        return false;
    };
    let [root, alias, ..] = path else {
        return false;
    };
    root.as_str() == Some("repository")
        && (0..count).any(|index| alias.as_str() == Some(divergence_alias(index).as_str()))
}

// --- updating a branch from its base -------------------------------------

/// How a branch behind its base is caught up.
///
/// REST's `PUT /pulls/{n}/update-branch` can only merge. The GraphQL mutation
/// takes `updateMethod`, and rebase is the half of the choice that keeps a
/// linear history, so the mutation is the only usable form of this operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchUpdateMethod {
    Merge,
    Rebase,
}

impl BranchUpdateMethod {
    /// The `PullRequestBranchUpdateMethod` value. GraphQL enum values are bare
    /// identifiers rather than strings, which is why this travels as a variable
    /// instead of being interpolated into the document.
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Merge => "MERGE",
            Self::Rebase => "REBASE",
        }
    }

    /// Progressive label for the in-flight banner. It names the method because
    /// the two produce different histories, and the user picked one.
    pub fn progress_label(self) -> &'static str {
        match self {
            Self::Merge => "Updating from base (merge)",
            Self::Rebase => "Updating from base (rebase)",
        }
    }
}

/// Aliased to `payload` the same way the draft mutations above are, so the wire
/// type ([`UpdateBranchData`]) is named after the shape it decodes rather than
/// after the operation that produced it.
///
/// `expectedHeadOid` is what makes this safe to fire from an already-rendered
/// view: if the branch moved since the pull request was drawn, GitHub refuses
/// the mutation instead of rewriting work the user has not seen.
pub const UPDATE_PULL_REQUEST_BRANCH: &str = r#"
mutation($id: ID!, $oid: GitObjectID!, $method: PullRequestBranchUpdateMethod!) {
  payload: updatePullRequestBranch(
    input: {pullRequestId: $id, expectedHeadOid: $oid, updateMethod: $method}
  ) {
    pullRequest { headRefOid }
  }
}
"#;

#[derive(Debug, Deserialize)]
pub struct UpdateBranchData {
    /// `null` when the mutation failed; the `errors` array carries the reason.
    pub payload: Option<UpdateBranchPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBranchPayload {
    /// `null` when the viewer may read the mutation result but not the pull
    /// request itself, which GitHub permits.
    pub pull_request: Option<UpdatedBranchNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedBranchNode {
    /// The commit the update landed on. Always a new oid, since both methods
    /// move the head, so it never matches the `expectedHeadOid` that was sent.
    pub head_ref_oid: String,
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
            // Filled in by the follow-up divergence batch, never by the feed
            // query itself.
            base_divergence: None,
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
    // --- branch divergence -------------------------------------------------

    /// The shape GitHub answers with when both refs resolve. `behindBy` is the
    /// head's distance from the base, which is the direction `Divergence`
    /// records; a swap here would point every catch-up button the wrong way.
    #[test]
    fn a_resolved_comparison_decodes_with_both_counts() {
        let body = r#"{
          "data": {
            "repository": {
              "ref": { "compare": { "aheadBy": 1, "behindBy": 2, "status": "DIVERGED" } }
            }
          }
        }"#;
        let response: GraphQlResponse<DivergenceQueryData> =
            serde_json::from_str(body).expect("should decode");
        let divergence = response
            .data
            .expect("data present")
            .into_domain()
            .expect("comparison present");
        assert_eq!(divergence, Divergence::new(1, 2));
        assert_eq!(
            divergence.relation(),
            rostrum_core::Relation::Diverged {
                ahead: 1.try_into().expect("non-zero"),
                behind: 2.try_into().expect("non-zero"),
            }
        );
    }

    /// An identical pair still answers with a comparison, not with null, so the
    /// "nothing to do" case must be distinguishable from "could not compare".
    #[test]
    fn an_identical_pair_decodes_as_a_present_zero_divergence() {
        let body = r#"{
          "data": {
            "repository": {
              "ref": { "compare": { "aheadBy": 0, "behindBy": 0, "status": "IDENTICAL" } }
            }
          }
        }"#;
        let response: GraphQlResponse<DivergenceQueryData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(
            response.data.expect("data present").into_domain(),
            Some(Divergence::IDENTICAL)
        );
    }

    /// The cross-fork case: the base repository cannot resolve the head ref, so
    /// `compare` is null. Absent is not a failure — it is the signal to compare
    /// against a local clone instead.
    #[test]
    fn an_unresolvable_head_ref_decodes_as_absent() {
        let body = r#"{"data":{"repository":{"ref":{"compare":null}}}}"#;
        let response: GraphQlResponse<DivergenceQueryData> =
            serde_json::from_str(body).expect("should decode");
        assert!(response.data.expect("data present").into_domain().is_none());
    }

    /// GitHub reports that same unresolvable head as HTTP 200 with a null
    /// `compare` *and* a `NOT_FOUND` error entry. Both halves must decode, so
    /// the client can recognise the pair rather than treating it as a hard
    /// failure.
    #[test]
    fn a_not_found_entry_accompanies_the_null_comparison() {
        let body = r#"{
          "data": { "repository": { "ref": { "compare": null } } },
          "errors": [{
            "type": "NOT_FOUND",
            "path": ["repository", "ref", "compare"],
            "message": "Could not resolve to a Ref with the name 'fork:feature'."
          }]
        }"#;
        let response: GraphQlResponse<DivergenceQueryData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(response.errors.len(), 1);
        assert!(
            response
                .errors
                .iter()
                .all(|e| e.kind.as_deref() == Some("NOT_FOUND")),
            "the client keys its fallback off every entry being NOT_FOUND"
        );
        assert!(response.data.expect("data present").into_domain().is_none());
    }

    /// A base branch deleted out from under an open pull request nulls the ref
    /// one level higher up; that must collapse to the same absent answer.
    #[test]
    fn a_missing_base_ref_decodes_as_absent() {
        for body in [
            r#"{"data":{"repository":{"ref":null}}}"#,
            r#"{"data":{"repository":null}}"#,
        ] {
            let response: GraphQlResponse<DivergenceQueryData> =
                serde_json::from_str(body).expect("should decode");
            assert!(
                response.data.expect("data present").into_domain().is_none(),
                "{body}"
            );
        }
    }

    #[test]
    fn the_divergence_query_compares_the_base_against_the_head() {
        assert!(PULL_REQUEST_DIVERGENCE.contains("ref(qualifiedName: $base)"));
        assert!(PULL_REQUEST_DIVERGENCE.contains("compare(headRef: $head)"));
    }

    // --- batched divergence ------------------------------------------------

    /// One alias and one variable pair per pull request, and nothing else
    /// varies with the count: the document is a function of the number of
    /// pairs alone, never of the branch names.
    #[test]
    fn the_batch_document_declares_one_variable_pair_per_alias() {
        let one = build_divergence_batch(1);
        assert!(one.contains("$b0: String!, $h0: String!"), "{one}");
        assert!(
            one.contains(
                "p0: ref(qualifiedName: $b0) { compare(headRef: $h0) { aheadBy behindBy } }"
            ),
            "{one}"
        );
        assert!(!one.contains("$b1"), "{one}");
        assert!(!one.contains("p1:"), "{one}");

        let three = build_divergence_batch(3);
        for index in 0..3 {
            assert!(three.contains(&format!("$b{index}: String!")), "{three}");
            assert!(three.contains(&format!("$h{index}: String!")), "{three}");
            assert!(
                three.contains(&format!(
                    "p{index}: ref(qualifiedName: $b{index}) {{ compare(headRef: $h{index})"
                )),
                "{three}"
            );
        }
        assert_eq!(three.matches(": ref(qualifiedName:").count(), 3);
        assert_eq!(three.matches("String!").count(), 2 + 3 * 2);
        assert!(three.contains("repository(owner: $owner, name: $name)"));
    }

    /// Every variable the document declares is one the variables object
    /// supplies, and vice versa — the two are built separately, and GitHub
    /// rejects a document with an unbound variable.
    #[test]
    fn the_batch_variables_match_the_declarations() {
        let pairs = vec![
            ("main".to_string(), "feature".to_string()),
            ("release/2".to_string(), "fix \"quoted\"".to_string()),
        ];
        let document = build_divergence_batch(pairs.len());
        let variables = divergence_batch_variables("a", "b", &pairs);
        let object = variables.as_object().expect("variables are an object");

        for name in ["owner", "name", "b0", "h0", "b1", "h1"] {
            assert!(object.contains_key(name), "missing {name}");
            assert!(
                document.contains(&format!("${name}: String!")),
                "{name} undeclared"
            );
        }
        assert_eq!(object.len(), 6);
        assert_eq!(object["b1"], "release/2");
        // Branch names never reach the document text.
        assert!(!document.contains("feature"));
        assert!(!document.contains("quoted"));
        assert_eq!(object["h1"], "fix \"quoted\"");
    }

    /// The batch answers with one alias per pair and the client reads them
    /// back by position; a partially unresolvable batch must keep the
    /// positions of the aliases that did resolve.
    #[test]
    fn a_batch_response_decodes_index_aligned_with_nulls_where_declined() {
        let body = r#"{
          "data": {
            "repository": {
              "p0": { "compare": { "aheadBy": 3, "behindBy": 0 } },
              "p1": { "compare": null },
              "p2": null,
              "p3": { "compare": { "aheadBy": 0, "behindBy": 7 } }
            }
          },
          "errors": [{
            "type": "NOT_FOUND",
            "path": ["repository", "p1", "compare"],
            "message": "Could not resolve to a Ref with the name 'fork:feature'."
          }]
        }"#;
        let response: GraphQlResponse<DivergenceBatchData> =
            serde_json::from_str(body).expect("should decode");
        let divergences = response.data.expect("data present").into_domain(4);
        assert_eq!(
            divergences,
            vec![
                Some(Divergence::new(3, 0)),
                None,
                None,
                Some(Divergence::new(0, 7)),
            ]
        );
        assert!(unexcused_batch_errors(response.errors, 4).is_empty());
    }

    /// A repository GitHub will not show answers `null` at the top, which
    /// must fan out to one `None` per pair rather than a shorter vector.
    #[test]
    fn a_withheld_repository_yields_one_absent_answer_per_pair() {
        let body = r#"{"data":{"repository":null}}"#;
        let response: GraphQlResponse<DivergenceBatchData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(
            response.data.expect("data present").into_domain(3),
            vec![None, None, None]
        );
    }

    /// Only a `NOT_FOUND` scoped to one of this batch's aliases is the
    /// expected cross-fork answer. Anything else must reach the caller.
    #[test]
    fn only_alias_scoped_not_founds_are_excused() {
        fn error(kind: Option<&str>, path: Option<&[&str]>) -> GraphQlError {
            GraphQlError {
                message: "m".into(),
                path: path.map(|p| p.iter().map(|s| serde_json::Value::from(*s)).collect()),
                kind: kind.map(str::to_string),
            }
        }

        let excused = [
            error(Some("NOT_FOUND"), Some(&["repository", "p0", "compare"])),
            error(Some("NOT_FOUND"), Some(&["repository", "p2"])),
        ];
        assert!(unexcused_batch_errors(excused.to_vec(), 3).is_empty());

        let kept = [
            // The repository itself: no alias was answered.
            error(Some("NOT_FOUND"), Some(&["repository"])),
            error(Some("NOT_FOUND"), None),
            // An alias this batch did not send.
            error(Some("NOT_FOUND"), Some(&["repository", "p3", "compare"])),
            // Right shape, wrong kind.
            error(Some("FORBIDDEN"), Some(&["repository", "p0", "compare"])),
            error(None, Some(&["repository", "p0", "compare"])),
        ];
        let unexcused = unexcused_batch_errors(kept.to_vec(), 3);
        assert_eq!(unexcused.len(), kept.len());

        let mixed: Vec<GraphQlError> = excused.iter().chain(kept.iter()).cloned().collect();
        assert_eq!(unexcused_batch_errors(mixed, 3).len(), kept.len());
    }

    // --- updating a branch from its base -----------------------------------

    /// The wire spellings are the `PullRequestBranchUpdateMethod` enum values.
    /// Swapping them would rebase when the user asked to merge.
    #[test]
    fn each_update_method_selects_its_wire_string() {
        assert_eq!(BranchUpdateMethod::Merge.as_api_str(), "MERGE");
        assert_eq!(BranchUpdateMethod::Rebase.as_api_str(), "REBASE");
    }

    /// The banner names the method, because the two leave different histories.
    #[test]
    fn each_update_method_labels_its_own_banner() {
        assert!(BranchUpdateMethod::Merge.progress_label().contains("merge"));
        assert!(
            BranchUpdateMethod::Rebase
                .progress_label()
                .contains("rebase")
        );
    }

    /// The alias is what lets the wire type be named after the shape rather
    /// than the mutation, exactly as the draft pair does it.
    #[test]
    fn the_branch_update_mutation_aliases_its_payload() {
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("payload:"));
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("updatePullRequestBranch"));
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("headRefOid"));
    }

    /// Guarding against a branch that moved is the whole reason this uses the
    /// mutation rather than the merge-only REST endpoint, so the expected oid
    /// and the method must both reach the input.
    #[test]
    fn the_branch_update_mutation_sends_the_expected_head_and_method() {
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("expectedHeadOid: $oid"));
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("updateMethod: $method"));
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("$oid: GitObjectID!"));
        assert!(UPDATE_PULL_REQUEST_BRANCH.contains("$method: PullRequestBranchUpdateMethod!"));
    }

    #[test]
    fn the_branch_update_response_carries_the_new_head_oid() {
        let body = r#"{"data":{"payload":{"pullRequest":{"headRefOid":"f00dcafe"}}}}"#;
        let response: GraphQlResponse<UpdateBranchData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(
            response
                .data
                .expect("data present")
                .payload
                .expect("payload present")
                .pull_request
                .expect("pull request present")
                .head_ref_oid,
            "f00dcafe"
        );
    }

    /// A refused update — the head moved, or the branch conflicts — answers 200
    /// with a null payload and a populated `errors` array.
    #[test]
    fn a_refused_branch_update_carries_its_reason() {
        let body = r#"{
          "data": { "payload": null },
          "errors": [{ "message": "expected head oid does not match the current head" }]
        }"#;
        let response: GraphQlResponse<UpdateBranchData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(response.errors.len(), 1);
        assert!(response.data.expect("data key present").payload.is_none());
    }
}
