//! GraphQL documents for issues and the wire types they decode into.
//!
//! One wire type, [`IssueNode`], serves both documents. The feed query selects
//! the summary fields; the detail query selects the same fields plus the body,
//! the comment nodes and the timeline. Fields only the detail query selects
//! are optional, so a feed node decodes into the same struct with them absent,
//! and the summary decoding — state, labels, assignees — exists exactly once.

use chrono::{DateTime, Utc};
use rostrum_core::{
    CommentId, Conversation, Issue, IssueDetail, IssueNumber, IssueState, Label, Milestone,
    NodeId, TimelineItem,
};
use serde::Deserialize;

use crate::{
    conversation::{IssueCommentNode, TimelineEventNode, close_reason},
    graphql::{AuthorNode, Connection, LabelNode, RateLimit},
};

/// Open issues for one repository, most recently updated first.
///
/// `issues` never returns pull requests — GitHub's GraphQL keeps the two
/// apart, unlike the REST `issues` list — so nothing here has to filter them
/// out. Two bounded connections per issue at ten entries each, the same shape
/// the pull request query uses; GitHub costs this at one point.
pub const OPEN_ISSUES: &str = r#"
query($owner: String!, $name: String!, $first: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    issues(states: OPEN, first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        id
        number
        title
        url
        state
        stateReason
        createdAt
        updatedAt
        author { login avatarUrl }
        assignees(first: 10) { nodes { login avatarUrl } }
        labels(first: 10) { nodes { name color } }
        comments { totalCount }
        milestone { title }
      }
    }
  }
}
"#;

/// Everything the issue pane shows, in one round trip.
///
/// `timelineItems` is restricted to the events the pane renders. Without an
/// `itemTypes` filter the connection also returns every comment, duplicating
/// the `comments` connection above it. `CrossReferencedEvent` is cheap — one
/// small `source` selection — and is the event that most often explains what
/// happened to an issue, so it is included.
pub const ISSUE_DETAIL: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      id
      number
      title
      url
      state
      stateReason
      body
      createdAt
      updatedAt
      author { login avatarUrl }
      assignees(first: 20) { nodes { login avatarUrl } }
      labels(first: 50) { nodes { name color } }
      milestone { title }
      comments(first: 100) {
        totalCount
        nodes { id body createdAt author { login avatarUrl } }
      }
      timelineItems(first: 100, itemTypes: [
        CLOSED_EVENT,
        REOPENED_EVENT,
        LABELED_EVENT,
        UNLABELED_EVENT,
        ASSIGNED_EVENT,
        UNASSIGNED_EVENT,
        RENAMED_TITLE_EVENT,
        CROSS_REFERENCED_EVENT
      ]) {
        nodes {
          __typename
          ... on ClosedEvent { createdAt actor { login avatarUrl } stateReason }
          ... on ReopenedEvent { createdAt actor { login avatarUrl } }
          ... on LabeledEvent { createdAt actor { login avatarUrl } label { name } }
          ... on UnlabeledEvent { createdAt actor { login avatarUrl } label { name } }
          ... on RenamedTitleEvent { createdAt actor { login avatarUrl } previousTitle currentTitle }
          ... on AssignedEvent {
            createdAt
            actor { login avatarUrl }
            assignee {
              ... on User { login }
              ... on Bot { login }
              ... on Mannequin { login }
              ... on Organization { login }
            }
          }
          ... on UnassignedEvent {
            createdAt
            actor { login avatarUrl }
            assignee {
              ... on User { login }
              ... on Bot { login }
              ... on Mannequin { login }
              ... on Organization { login }
            }
          }
          ... on CrossReferencedEvent {
            createdAt
            actor { login avatarUrl }
            source {
              __typename
              ... on Issue { number title repository { nameWithOwner } }
              ... on PullRequest { number title repository { nameWithOwner } }
            }
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenIssuesData {
    pub rate_limit: Option<RateLimit>,
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<IssuesRepositoryNode>,
}

#[derive(Debug, Deserialize)]
pub struct IssuesRepositoryNode {
    /// `null` when the repository has issues disabled — not an error, just a
    /// repository with nothing to list.
    pub issues: Option<Connection<IssueNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetailData {
    pub rate_limit: Option<RateLimit>,
    pub repository: Option<IssueDetailRepository>,
}

#[derive(Debug, Deserialize)]
pub struct IssueDetailRepository {
    /// `null`, with a `NOT_FOUND` error, when the number is not an issue —
    /// including when it is a pull request.
    pub issue: Option<IssueNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueNode {
    pub id: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    /// `OPEN` or `CLOSED`, read leniently: see [`issue_state`].
    #[serde(default)]
    pub state: Option<String>,
    /// `COMPLETED`, `NOT_PLANNED`, `DUPLICATE`, or `REOPENED` on an open
    /// issue that was once closed.
    #[serde(default)]
    pub state_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// `null` for deleted accounts and some bot actors.
    pub author: Option<AuthorNode>,
    pub assignees: Option<Connection<AuthorNode>>,
    pub labels: Option<Connection<LabelNode>>,
    pub comments: Option<IssueComments>,
    pub milestone: Option<MilestoneNode>,
    /// Detail query only.
    #[serde(default)]
    pub body: Option<String>,
    /// Detail query only.
    #[serde(default)]
    pub timeline_items: Option<Connection<TimelineEventNode>>,
}

/// `comments`: the count in both documents, the nodes in the detail one.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueComments {
    #[serde(default)]
    pub total_count: u32,
    pub nodes: Option<Vec<Option<IssueCommentNode>>>,
}

#[derive(Debug, Deserialize)]
pub struct MilestoneNode {
    pub title: String,
}

/// Collapse `state` and `stateReason` into the one [`IssueState`] the model
/// allows.
///
/// Anything but `CLOSED` reads as open: the feed asks for open issues only,
/// and a state value GitHub adds later should cost an accurate chip, not the
/// whole repository's decode. A reason survives only on a closed issue —
/// `REOPENED` on an open one is history, not a state.
pub fn issue_state(state: Option<&str>, reason: Option<&str>) -> IssueState {
    match state {
        Some("CLOSED") => IssueState::Closed(reason.and_then(close_reason)),
        _ => IssueState::Open,
    }
}

impl IssueNode {
    /// The summary the feed lists.
    pub fn into_domain(self) -> Issue {
        self.split().0
    }

    /// The summary plus the timeline, for the issue pane.
    pub fn into_detail(self) -> IssueDetail {
        let (issue, body, comments, events) = self.split();

        let mut items = vec![TimelineItem::Body {
            author: issue.author.clone(),
            body: body.unwrap_or_default(),
            created_at: issue.created_at,
        }];
        items.extend(comments.into_iter().flatten().map(|comment| {
            TimelineItem::Comment {
                id: CommentId(comment.id),
                author: comment.author.and_then(AuthorNode::into_user),
                body: comment.body,
                created_at: comment.created_at,
            }
        }));
        items.extend(
            events
                .map(Connection::into_vec)
                .unwrap_or_default()
                .into_iter()
                .filter_map(TimelineEventNode::into_domain),
        );

        let mut conversation = Conversation {
            items,
            ..Default::default()
        };
        conversation.sort();
        IssueDetail {
            issue,
            conversation,
        }
    }

    #[allow(clippy::type_complexity)]
    fn split(
        self,
    ) -> (
        Issue,
        Option<String>,
        Vec<Option<IssueCommentNode>>,
        Option<Connection<TimelineEventNode>>,
    ) {
        let (comment_count, comment_nodes) = match self.comments {
            Some(comments) => (comments.total_count, comments.nodes.unwrap_or_default()),
            None => (0, Vec::new()),
        };
        let issue = Issue {
            number: IssueNumber(self.number),
            node_id: NodeId(self.id),
            title: self.title,
            url: self.url,
            state: issue_state(self.state.as_deref(), self.state_reason.as_deref()),
            created_at: self.created_at,
            updated_at: self.updated_at,
            author: self.author.and_then(AuthorNode::into_user),
            assignees: self
                .assignees
                .map(Connection::into_vec)
                .unwrap_or_default()
                .into_iter()
                .filter_map(AuthorNode::into_user)
                .collect(),
            labels: self
                .labels
                .map(Connection::into_vec)
                .unwrap_or_default()
                .into_iter()
                .map(|label| Label {
                    name: label.name,
                    color: label.color,
                })
                .collect(),
            comment_count,
            milestone: self.milestone.map(|m| Milestone { title: m.title }),
        };
        (issue, self.body, comment_nodes, self.timeline_items)
    }
}

impl OpenIssuesData {
    /// The listed issues, or `None` when the repository itself was withheld.
    /// A repository with issues disabled yields an empty list.
    pub fn into_domain(self) -> Option<Vec<Issue>> {
        let repository = self.repository?;
        Some(
            repository
                .issues
                .map(Connection::into_vec)
                .unwrap_or_default()
                .into_iter()
                .map(IssueNode::into_domain)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{CloseReason, EventKind};

    use super::*;
    use crate::graphql::GraphQlResponse;

    // Captured from the live API with the documents above, unedited apart
    // from pretty-printing.
    const ZED: &str = include_str!("../../fixtures/issues/open_issues_zed.json");
    const ASSIGNED: &str = include_str!("../../fixtures/issues/open_issues_assigned.json");
    const MILESTONE: &str = include_str!("../../fixtures/issues/open_issues_milestone.json");
    const CROSS_REFERENCED: &str = include_str!("../../fixtures/issues/detail_cross_referenced.json");
    const DETAIL_ASSIGNED: &str = include_str!("../../fixtures/issues/detail_assigned.json");
    const NOT_PLANNED: &str = include_str!("../../fixtures/issues/detail_closed_not_planned.json");
    const REOPENED: &str = include_str!("../../fixtures/issues/detail_reopened.json");

    fn feed(body: &str) -> Vec<Issue> {
        let response: GraphQlResponse<OpenIssuesData> =
            serde_json::from_str(body).expect("fixture should decode");
        assert!(response.errors.is_empty());
        response
            .data
            .expect("data present")
            .into_domain()
            .expect("repository present")
    }

    fn detail(body: &str) -> IssueDetail {
        let response: GraphQlResponse<IssueDetailData> =
            serde_json::from_str(body).expect("fixture should decode");
        assert!(response.errors.is_empty());
        response
            .data
            .expect("data present")
            .repository
            .expect("repository present")
            .issue
            .expect("issue present")
            .into_detail()
    }

    fn events(detail: &IssueDetail) -> Vec<EventKind> {
        detail
            .conversation
            .items
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Event { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn decodes_a_captured_feed_of_open_issues() {
        let issues = feed(ZED);
        assert_eq!(issues.len(), 4);
        let first = &issues[0];
        assert_eq!(first.number, IssueNumber(65045));
        assert_eq!(first.node_id, NodeId("I_kwDOFExXwM8AAAABUbRfPw".into()));
        assert_eq!(first.state, IssueState::Open);
        assert_eq!(
            first.author.as_ref().map(|a| a.login.as_str()),
            Some("dmitry-kuznetsov")
        );
        assert_eq!(
            first.labels,
            vec![Label {
                name: "state:needs triage".into(),
                color: "fbca04".into()
            }]
        );
        assert_eq!(first.comment_count, 1);
        assert!(first.assignees.is_empty());
        assert!(first.milestone.is_none());
        assert!(
            first
                .url
                .starts_with("https://github.com/zed-industries/zed/issues/")
        );
        // Most recently updated first, as the document orders them.
        assert!(
            issues
                .windows(2)
                .all(|pair| pair[0].updated_at >= pair[1].updated_at)
        );
    }

    #[test]
    fn decodes_assignees_from_a_captured_feed() {
        let issues = feed(ASSIGNED);
        let assigned = issues
            .iter()
            .find(|issue| issue.number == IssueNumber(163577))
            .expect("fixture holds #163577");
        assert_eq!(assigned.assignees.len(), 1);
        assert!(issues.iter().filter(|i| i.assignees.is_empty()).count() >= 1);
    }

    #[test]
    fn decodes_milestones_from_a_captured_feed() {
        let issues = feed(MILESTONE);
        assert_eq!(
            issues[0].milestone,
            Some(Milestone {
                title: "1.99.0".into()
            })
        );
        assert_eq!(issues[0].assignees.len(), 1);
    }

    #[test]
    fn a_detail_carries_the_issue_and_a_sorted_timeline() {
        let detail = detail(CROSS_REFERENCED);
        assert_eq!(detail.issue.number, IssueNumber(160793));
        assert_eq!(detail.issue.state, IssueState::Open);
        assert_eq!(detail.issue.comment_count, 7);

        let items = &detail.conversation.items;
        let TimelineItem::Body { body, .. } = &items[0] else {
            panic!("body first, got {:?}", items[0]);
        };
        assert!(!body.is_empty());
        let comments = items
            .iter()
            .filter(|item| matches!(item, TimelineItem::Comment { .. }))
            .count();
        assert_eq!(comments, 7);
        let stamps: Vec<_> = items[1..].iter().map(TimelineItem::created_at).collect();
        assert!(stamps.windows(2).all(|w| w[0] <= w[1]), "not sorted");
        // Issues have neither review threads nor checks.
        assert!(detail.conversation.threads.is_empty());
        assert!(detail.conversation.checks.is_empty());
    }

    #[test]
    fn cross_references_name_their_source() {
        let detail = detail(CROSS_REFERENCED);
        let events = events(&detail);
        assert!(events.contains(&EventKind::CrossReferenced {
            source: "rust-lang/rust#160794".into(),
            title: "`thread::current` may indirectly call `Arc::new_uninit_in(System)` which may call an untrusted alloc error hook".into(),
        }));
        assert!(events.iter().any(|e| matches!(e, EventKind::Labeled { .. })));
        assert!(events.iter().any(|e| matches!(e, EventKind::Unlabeled { .. })));
        assert!(events.iter().any(|e| matches!(e, EventKind::Renamed { .. })));
        assert!(!events.iter().any(|e| matches!(e, EventKind::Other(_))));
    }

    #[test]
    fn assignment_events_name_the_assignee() {
        let detail = detail(DETAIL_ASSIGNED);
        assert!(events(&detail).contains(&EventKind::Assigned {
            assignee: "maxdexh".into()
        }));
        assert_eq!(detail.issue.assignees.len(), 1);
    }

    #[test]
    fn a_closed_issue_carries_its_reason_in_state_and_timeline() {
        let detail = detail(NOT_PLANNED);
        assert_eq!(
            detail.issue.state,
            IssueState::Closed(Some(CloseReason::NotPlanned))
        );
        assert!(events(&detail).contains(&EventKind::ClosedAs(CloseReason::NotPlanned)));
    }

    /// `REOPENED` is the reason GitHub reports on an open issue that was once
    /// closed. It is history, so the state is plain open, and the timeline
    /// keeps both the close (with its reason) and the reopen.
    #[test]
    fn a_reopened_issue_is_open_with_its_history_intact() {
        let detail = detail(REOPENED);
        assert_eq!(detail.issue.state, IssueState::Open);
        let events = events(&detail);
        let closed = events
            .iter()
            .position(|e| *e == EventKind::ClosedAs(CloseReason::Completed))
            .expect("close event present");
        let reopened = events
            .iter()
            .position(|e| *e == EventKind::Reopened)
            .expect("reopen event present");
        assert!(closed < reopened);
    }

    #[test]
    fn state_and_reason_collapse_into_one_state() {
        for (state, reason, expected) in [
            (Some("OPEN"), None, IssueState::Open),
            (Some("OPEN"), Some("REOPENED"), IssueState::Open),
            (Some("CLOSED"), None, IssueState::Closed(None)),
            (
                Some("CLOSED"),
                Some("COMPLETED"),
                IssueState::Closed(Some(CloseReason::Completed)),
            ),
            (
                Some("CLOSED"),
                Some("DUPLICATE"),
                IssueState::Closed(Some(CloseReason::Duplicate)),
            ),
            (Some("CLOSED"), Some("SOMETHING_NEW"), IssueState::Closed(None)),
            (Some("ARCHIVED"), None, IssueState::Open),
            (None, None, IssueState::Open),
        ] {
            assert_eq!(issue_state(state, reason), expected, "{state:?} {reason:?}");
        }
    }

    /// Nulls GitHub routinely sends — a deleted author, null connections, a
    /// repository with issues disabled — must not fail the decode.
    #[test]
    fn tolerates_nulls_and_disabled_issues() {
        let body = r#"{"data":{"rateLimit":null,"repository":{"issues":{"nodes":[
          null,
          {"id":"I_1","number":1,"title":"t","url":"u","state":"OPEN","stateReason":null,
           "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z",
           "author":null,"assignees":null,"labels":{"nodes":null},"comments":null,"milestone":null}
        ]}}}}"#;
        let issues = feed(body);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].author.is_none());
        assert!(issues[0].labels.is_empty());
        assert_eq!(issues[0].comment_count, 0);

        let disabled = r#"{"data":{"rateLimit":null,"repository":{"issues":null}}}"#;
        assert!(feed(disabled).is_empty());

        let withheld: GraphQlResponse<OpenIssuesData> =
            serde_json::from_str(r#"{"data":{"rateLimit":null,"repository":null}}"#)
                .expect("decodes");
        assert!(withheld.data.expect("data").into_domain().is_none());
    }

    #[test]
    fn the_feed_document_asks_for_open_issues_with_every_listed_field() {
        assert!(OPEN_ISSUES.contains("issues(states: OPEN"));
        for field in [
            "id", "number", "title", "state", "stateReason", "createdAt", "updatedAt",
            "author", "assignees", "labels", "color", "comments { totalCount }", "milestone",
        ] {
            assert!(OPEN_ISSUES.contains(field), "{field}");
        }
    }
}
