//! The single GraphQL query behind the pull request detail pane.
//!
//! One round trip returns the description, issue comments, reviews, inline
//! review threads, timeline events, and the check rollup. Fetching these
//! separately would cost six requests per pull request and still race, since the
//! pieces reference each other by id.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rostrum_core::{
    CheckRun, CheckState, CommentId, Conversation, EventKind, ReviewId, ReviewState, ReviewThread,
    Side, ThreadComment, ThreadId, TimelineItem, User,
};
use serde::Deserialize;

use crate::graphql::{AuthorNode, Connection, RateLimit};

/// Everything the detail pane needs for one pull request.
///
/// `timelineItems` is restricted to the event types the UI renders; without an
/// `itemTypes` filter the connection also returns every comment and review,
/// which would duplicate the dedicated connections above it.
pub const PULL_REQUEST_CONVERSATION: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      body
      createdAt
      author { login avatarUrl }
      comments(first: 100) {
        nodes { id body createdAt author { login avatarUrl } }
      }
      reviews(first: 100) {
        nodes { id body state createdAt author { login avatarUrl } }
      }
      reviewThreads(first: 100) {
        nodes {
          id
          path
          line
          originalLine
          diffSide
          isResolved
          isOutdated
          comments(first: 50) {
            nodes {
              id
              databaseId
              body
              createdAt
              author { login avatarUrl }
              pullRequestReview { id }
            }
          }
        }
      }
      timelineItems(first: 100, itemTypes: [
        MERGED_EVENT,
        CLOSED_EVENT,
        REOPENED_EVENT,
        READY_FOR_REVIEW_EVENT,
        CONVERT_TO_DRAFT_EVENT,
        HEAD_REF_FORCE_PUSHED_EVENT,
        REVIEW_REQUESTED_EVENT,
        ASSIGNED_EVENT,
        LABELED_EVENT,
        UNLABELED_EVENT,
        RENAMED_TITLE_EVENT
      ]) {
        nodes {
          __typename
          ... on MergedEvent { createdAt actor { login avatarUrl } }
          ... on ClosedEvent { createdAt actor { login avatarUrl } }
          ... on ReopenedEvent { createdAt actor { login avatarUrl } }
          ... on ReadyForReviewEvent { createdAt actor { login avatarUrl } }
          ... on ConvertToDraftEvent { createdAt actor { login avatarUrl } }
          ... on HeadRefForcePushedEvent { createdAt actor { login avatarUrl } }
          ... on LabeledEvent { createdAt actor { login avatarUrl } label { name } }
          ... on UnlabeledEvent { createdAt actor { login avatarUrl } label { name } }
          ... on RenamedTitleEvent { createdAt actor { login avatarUrl } previousTitle currentTitle }
          ... on ReviewRequestedEvent {
            createdAt
            actor { login avatarUrl }
            requestedReviewer {
              ... on User { login }
              ... on Bot { login }
              ... on Mannequin { login }
              ... on Team { name }
            }
          }
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
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              contexts(first: 100) {
                nodes {
                  __typename
                  ... on CheckRun { name conclusion status detailsUrl }
                  ... on StatusContext { context state targetUrl }
                }
              }
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
pub struct ConversationQueryData {
    pub rate_limit: Option<RateLimit>,
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<ConversationRepository>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRepository {
    /// `null` when the pull request number does not exist.
    pub pull_request: Option<ConversationNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationNode {
    /// Non-null in the schema, but empty for a pull request opened with no
    /// description, so the default keeps that from being an error case.
    #[serde(default)]
    pub body: String,
    pub created_at: DateTime<Utc>,
    /// `null` for deleted accounts and some bot actors.
    pub author: Option<AuthorNode>,
    pub comments: Option<Connection<IssueCommentNode>>,
    pub reviews: Option<Connection<ReviewNode>>,
    pub review_threads: Option<Connection<ReviewThreadNode>>,
    pub timeline_items: Option<Connection<TimelineEventNode>>,
    pub commits: Option<Connection<RollupCommitEdge>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommentNode {
    pub id: String,
    #[serde(default)]
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub author: Option<AuthorNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewNode {
    pub id: String,
    #[serde(default)]
    pub body: String,
    pub state: ReviewState,
    pub created_at: DateTime<Utc>,
    pub author: Option<AuthorNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewThreadNode {
    pub id: String,
    #[serde(default)]
    pub path: String,
    /// `null` once the thread is outdated: the line it referred to no longer
    /// exists in the current diff.
    pub line: Option<u32>,
    pub original_line: Option<u32>,
    /// `null` is not expected, but a thread that lost its side is still worth
    /// showing; it defaults to the new-file side.
    pub diff_side: Option<Side>,
    #[serde(default)]
    pub is_resolved: bool,
    #[serde(default)]
    pub is_outdated: bool,
    pub comments: Option<Connection<ThreadCommentNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCommentNode {
    pub id: String,
    /// The REST id. Replying into a thread is REST-only, and that endpoint
    /// rejects the GraphQL node id.
    pub database_id: Option<u64>,
    #[serde(default)]
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub author: Option<AuthorNode>,
    /// The review that introduced this comment. Used to hang the thread off the
    /// right review in the timeline.
    pub pull_request_review: Option<ReviewRef>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewRef {
    pub id: String,
}

/// A timeline entry, decoded structurally rather than as a tagged enum.
///
/// `#[serde(tag = "__typename")]` would reject any type GitHub adds later, and
/// its `#[serde(other)]` fallback cannot capture the type name we need for
/// [`EventKind::Other`]. Reading the union of all selected fields instead means
/// an unrecognised event degrades to `Other` rather than failing the decode.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEventNode {
    #[serde(rename = "__typename", default)]
    pub typename: String,
    /// Absent only for an event type this query did not select fields for.
    pub created_at: Option<DateTime<Utc>>,
    pub actor: Option<AuthorNode>,
    pub label: Option<LabelName>,
    pub previous_title: Option<String>,
    pub current_title: Option<String>,
    pub requested_reviewer: Option<ActorRef>,
    pub assignee: Option<ActorRef>,
}

#[derive(Debug, Deserialize)]
pub struct LabelName {
    pub name: String,
}

/// A member of one of GitHub's actor unions. `User`/`Bot`/`Mannequin` carry
/// `login`; `Team` carries `name`.
#[derive(Debug, Deserialize)]
pub struct ActorRef {
    pub login: Option<String>,
    pub name: Option<String>,
}

impl ActorRef {
    fn display(&self) -> Option<&str> {
        self.login.as_deref().or(self.name.as_deref())
    }
}

#[derive(Debug, Deserialize)]
pub struct RollupCommitEdge {
    pub commit: RollupCommitNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollupCommitNode {
    /// `null` when no CI is configured for the head commit.
    pub status_check_rollup: Option<ContextRollup>,
}

#[derive(Debug, Deserialize)]
pub struct ContextRollup {
    pub contexts: Option<Connection<CheckContextNode>>,
}

/// A `StatusCheckRollupContext`, which is either a `CheckRun` (GitHub Actions
/// and friends) or a `StatusContext` (the older commit status API). The two
/// spell their name, state, and link differently.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckContextNode {
    #[serde(rename = "__typename", default)]
    pub typename: String,
    // CheckRun
    pub name: Option<String>,
    pub conclusion: Option<String>,
    pub status: Option<String>,
    pub details_url: Option<String>,
    // StatusContext
    pub context: Option<String>,
    pub state: Option<CheckState>,
    pub target_url: Option<String>,
}

fn into_user(author: AuthorNode) -> User {
    User {
        login: author.login,
        avatar_url: author.avatar_url,
    }
}

/// Collapse a check run's `conclusion`/`status` pair onto the same scale the
/// older status API uses, so both kinds of check render identically.
///
/// A completed run with no conclusion, and a conclusion GitHub adds later, both
/// become `None` — "no opinion" rather than a guessed failure.
fn check_run_state(conclusion: Option<&str>, status: Option<&str>) -> Option<CheckState> {
    match conclusion {
        Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => Some(CheckState::Success),
        Some("FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE" | "CANCELLED") => {
            Some(CheckState::Failure)
        }
        Some("ACTION_REQUIRED" | "STALE") => Some(CheckState::Error),
        Some(_) => None,
        None => match status {
            Some("QUEUED" | "IN_PROGRESS" | "WAITING" | "PENDING" | "REQUESTED") => {
                Some(CheckState::Pending)
            }
            _ => None,
        },
    }
}

impl CheckContextNode {
    fn into_domain(self) -> Option<CheckRun> {
        match self.typename.as_str() {
            "CheckRun" => Some(CheckRun {
                name: self.name.unwrap_or_default(),
                state: check_run_state(self.conclusion.as_deref(), self.status.as_deref()),
                url: self.details_url,
            }),
            "StatusContext" => Some(CheckRun {
                name: self.context.unwrap_or_default(),
                state: self.state,
                url: self.target_url,
            }),
            // A rollup context that is neither shape has no name to show.
            _ => None,
        }
    }
}

impl TimelineEventNode {
    /// `None` when the event carries no timestamp, since an item with no
    /// position on the timeline cannot be placed.
    fn into_domain(self) -> Option<TimelineItem> {
        let created_at = self.created_at?;
        let kind = match self.typename.as_str() {
            "MergedEvent" => EventKind::Merged,
            "ClosedEvent" => EventKind::Closed,
            "ReopenedEvent" => EventKind::Reopened,
            "ReadyForReviewEvent" => EventKind::ReadyForReview,
            "ConvertToDraftEvent" => EventKind::ConvertedToDraft,
            "HeadRefForcePushedEvent" => EventKind::HeadRefForcePushed,
            "LabeledEvent" => EventKind::Labeled {
                name: self.label.map(|l| l.name).unwrap_or_default(),
            },
            "UnlabeledEvent" => EventKind::Unlabeled {
                name: self.label.map(|l| l.name).unwrap_or_default(),
            },
            "RenamedTitleEvent" => EventKind::Renamed {
                from: self.previous_title.unwrap_or_default(),
                to: self.current_title.unwrap_or_default(),
            },
            "ReviewRequestedEvent" => EventKind::ReviewRequested {
                reviewer: self
                    .requested_reviewer
                    .as_ref()
                    .and_then(ActorRef::display)
                    .unwrap_or_default()
                    .to_string(),
            },
            "AssignedEvent" => EventKind::Assigned {
                assignee: self
                    .assignee
                    .as_ref()
                    .and_then(ActorRef::display)
                    .unwrap_or_default()
                    .to_string(),
            },
            other => EventKind::Other(other.to_string()),
        };

        Some(TimelineItem::Event {
            kind,
            actor: self.actor.map(into_user),
            created_at,
        })
    }
}

impl ReviewThreadNode {
    /// Returns the thread plus the id of the review that opened it, taken from
    /// the review its first comment belongs to.
    fn into_domain(self) -> (ReviewThread, Option<String>) {
        let comments = self
            .comments
            .map(Connection::into_vec)
            .unwrap_or_default()
            .into_iter();

        let mut opening_review = None;
        let comments: Vec<ThreadComment> = comments
            .enumerate()
            .map(|(index, comment)| {
                if index == 0 {
                    opening_review = comment.pull_request_review.map(|review| review.id);
                }
                ThreadComment {
                    id: CommentId(comment.id),
                    database_id: comment.database_id,
                    author: comment.author.map(into_user),
                    body: comment.body,
                    created_at: comment.created_at,
                }
            })
            .collect();

        let thread = ReviewThread {
            id: ThreadId(self.id),
            path: self.path,
            line: self.line,
            original_line: self.original_line,
            side: self.diff_side.unwrap_or(Side::Right),
            is_resolved: self.is_resolved,
            is_outdated: self.is_outdated,
            comments,
        };
        (thread, opening_review)
    }
}

impl ConversationNode {
    pub fn into_domain(self) -> Conversation {
        let mut items = vec![TimelineItem::Body {
            author: self.author.map(into_user),
            body: self.body,
            created_at: self.created_at,
        }];

        for comment in self.comments.map(Connection::into_vec).unwrap_or_default() {
            items.push(TimelineItem::Comment {
                id: CommentId(comment.id),
                author: comment.author.map(into_user),
                body: comment.body,
                created_at: comment.created_at,
            });
        }

        // Threads are decoded before reviews so each review can be given the
        // ids of the threads it opened.
        let mut threads = Vec::new();
        let mut threads_by_review: HashMap<String, Vec<ThreadId>> = HashMap::new();
        for node in self
            .review_threads
            .map(Connection::into_vec)
            .unwrap_or_default()
        {
            let (thread, opening_review) = node.into_domain();
            if let Some(review_id) = opening_review {
                threads_by_review
                    .entry(review_id)
                    .or_default()
                    .push(thread.id.clone());
            }
            threads.push(thread);
        }

        for review in self.reviews.map(Connection::into_vec).unwrap_or_default() {
            let thread_ids = threads_by_review.remove(&review.id).unwrap_or_default();
            items.push(TimelineItem::Review {
                id: ReviewId(review.id),
                author: review.author.map(into_user),
                state: review.state,
                body: review.body,
                created_at: review.created_at,
                thread_ids,
            });
        }

        items.extend(
            self.timeline_items
                .map(Connection::into_vec)
                .unwrap_or_default()
                .into_iter()
                .filter_map(TimelineEventNode::into_domain),
        );

        let checks = self
            .commits
            .map(Connection::into_vec)
            .unwrap_or_default()
            .into_iter()
            .next()
            .and_then(|edge| edge.commit.status_check_rollup)
            .and_then(|rollup| rollup.contexts)
            .map(Connection::into_vec)
            .unwrap_or_default()
            .into_iter()
            .filter_map(CheckContextNode::into_domain)
            .collect();

        let mut conversation = Conversation {
            items,
            threads,
            checks,
        };
        conversation.sort();
        conversation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::GraphQlResponse;

    /// Shaped like a real response, including the shapes that only show up on
    /// busy pull requests: a resolved thread, a status-API check alongside a
    /// check run, and a timeline type this query does not know about.
    const SAMPLE: &str = r#"{
      "data": {
        "rateLimit": { "cost": 1, "remaining": 4998, "resetAt": "2026-08-01T12:00:00Z" },
        "repository": {
          "pullRequest": {
            "body": "Fixes the thing.",
            "createdAt": "2026-07-30T10:00:00Z",
            "author": { "login": "octocat", "avatarUrl": "https://example.invalid/a.png" },
            "comments": {
              "nodes": [
                {
                  "id": "IC_1",
                  "body": "first",
                  "createdAt": "2026-07-30T11:00:00Z",
                  "author": { "login": "reviewer", "avatarUrl": null }
                },
                {
                  "id": "IC_2",
                  "body": "second",
                  "createdAt": "2026-07-30T15:00:00Z",
                  "author": { "login": "octocat", "avatarUrl": null }
                }
              ]
            },
            "reviews": {
              "nodes": [
                {
                  "id": "PRR_1",
                  "body": "some notes",
                  "state": "CHANGES_REQUESTED",
                  "createdAt": "2026-07-30T12:00:00Z",
                  "author": { "login": "reviewer", "avatarUrl": null }
                },
                {
                  "id": "PRR_2",
                  "body": "",
                  "state": "APPROVED",
                  "createdAt": "2026-07-30T16:00:00Z",
                  "author": { "login": "reviewer", "avatarUrl": null }
                }
              ]
            },
            "reviewThreads": {
              "nodes": [
                {
                  "id": "PRRT_1",
                  "path": "src/main.rs",
                  "line": 12,
                  "originalLine": 12,
                  "diffSide": "RIGHT",
                  "isResolved": false,
                  "isOutdated": false,
                  "comments": {
                    "nodes": [
                      {
                        "id": "PRRC_1",
                        "databaseId": 900001,
                        "body": "rename this",
                        "createdAt": "2026-07-30T12:00:00Z",
                        "author": { "login": "reviewer", "avatarUrl": null },
                        "pullRequestReview": { "id": "PRR_1" }
                      },
                      {
                        "id": "PRRC_2",
                        "databaseId": 900002,
                        "body": "done",
                        "createdAt": "2026-07-30T13:00:00Z",
                        "author": { "login": "octocat", "avatarUrl": null },
                        "pullRequestReview": { "id": "PRR_3" }
                      }
                    ]
                  }
                },
                {
                  "id": "PRRT_2",
                  "path": "src/lib.rs",
                  "line": 4,
                  "originalLine": 4,
                  "diffSide": "LEFT",
                  "isResolved": true,
                  "isOutdated": false,
                  "comments": {
                    "nodes": [
                      {
                        "id": "PRRC_3",
                        "databaseId": 900003,
                        "body": "nit",
                        "createdAt": "2026-07-30T12:00:01Z",
                        "author": { "login": "reviewer", "avatarUrl": null },
                        "pullRequestReview": { "id": "PRR_1" }
                      }
                    ]
                  }
                }
              ]
            },
            "timelineItems": {
              "nodes": [
                {
                  "__typename": "LabeledEvent",
                  "createdAt": "2026-07-30T10:30:00Z",
                  "actor": { "login": "triage", "avatarUrl": null },
                  "label": { "name": "bug" }
                },
                {
                  "__typename": "ReviewRequestedEvent",
                  "createdAt": "2026-07-30T10:31:00Z",
                  "actor": { "login": "octocat", "avatarUrl": null },
                  "requestedReviewer": { "login": "reviewer" }
                },
                {
                  "__typename": "ReviewRequestedEvent",
                  "createdAt": "2026-07-30T10:32:00Z",
                  "actor": { "login": "octocat", "avatarUrl": null },
                  "requestedReviewer": { "name": "platform-team" }
                },
                {
                  "__typename": "RenamedTitleEvent",
                  "createdAt": "2026-07-30T14:00:00Z",
                  "actor": { "login": "octocat", "avatarUrl": null },
                  "previousTitle": "wip",
                  "currentTitle": "Fix the thing"
                },
                {
                  "__typename": "ReadyForReviewEvent",
                  "createdAt": "2026-07-30T14:30:00Z",
                  "actor": { "login": "octocat", "avatarUrl": null }
                },
                {
                  "__typename": "MergedEvent",
                  "createdAt": "2026-07-30T17:00:00Z",
                  "actor": { "login": "reviewer", "avatarUrl": null }
                },
                {
                  "__typename": "PinnedEvent",
                  "createdAt": "2026-07-30T17:30:00Z",
                  "actor": null
                }
              ]
            },
            "commits": {
              "nodes": [
                {
                  "commit": {
                    "statusCheckRollup": {
                      "contexts": {
                        "nodes": [
                          {
                            "__typename": "CheckRun",
                            "name": "build",
                            "conclusion": "SUCCESS",
                            "status": "COMPLETED",
                            "detailsUrl": "https://example.invalid/build"
                          },
                          {
                            "__typename": "CheckRun",
                            "name": "test",
                            "conclusion": null,
                            "status": "IN_PROGRESS",
                            "detailsUrl": "https://example.invalid/test"
                          },
                          {
                            "__typename": "StatusContext",
                            "context": "ci/legacy",
                            "state": "FAILURE",
                            "targetUrl": "https://example.invalid/legacy"
                          }
                        ]
                      }
                    }
                  }
                }
              ]
            }
          }
        }
      }
    }"#;

    fn parse(body: &str) -> Conversation {
        let response: GraphQlResponse<ConversationQueryData> =
            serde_json::from_str(body).expect("sample should decode");
        assert!(response.errors.is_empty());
        response
            .data
            .expect("data present")
            .repository
            .expect("repository present")
            .pull_request
            .expect("pull request present")
            .into_domain()
    }

    #[test]
    fn body_is_first_and_the_rest_is_chronological() {
        let conversation = parse(SAMPLE);

        let TimelineItem::Body { body, author, .. } = &conversation.items[0] else {
            panic!("expected body first, got {:?}", conversation.items[0]);
        };
        assert_eq!(body, "Fixes the thing.");
        assert_eq!(author.as_ref().map(|a| a.login.as_str()), Some("octocat"));

        let timestamps: Vec<_> = conversation.items[1..]
            .iter()
            .map(TimelineItem::created_at)
            .collect();
        assert!(
            timestamps.windows(2).all(|w| w[0] <= w[1]),
            "not sorted: {timestamps:?}"
        );
    }

    #[test]
    fn decodes_comments_reviews_and_events() {
        let conversation = parse(SAMPLE);

        let comments: Vec<_> = conversation
            .items
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Comment { id, body, .. } => Some((id.0.as_str(), body.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(comments, [("IC_1", "first"), ("IC_2", "second")]);

        let reviews: Vec<_> = conversation
            .items
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Review { id, state, .. } => Some((id.0.as_str(), *state)),
                _ => None,
            })
            .collect();
        assert_eq!(
            reviews,
            [
                ("PRR_1", ReviewState::ChangesRequested),
                ("PRR_2", ReviewState::Approved),
            ]
        );

        let events: Vec<_> = conversation
            .items
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Event { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            events,
            [
                EventKind::Labeled { name: "bug".into() },
                EventKind::ReviewRequested {
                    reviewer: "reviewer".into()
                },
                EventKind::ReviewRequested {
                    reviewer: "platform-team".into()
                },
                EventKind::Renamed {
                    from: "wip".into(),
                    to: "Fix the thing".into()
                },
                EventKind::ReadyForReview,
                EventKind::Merged,
                // An event type this query never asked for still decodes.
                EventKind::Other("PinnedEvent".into()),
            ]
        );
    }

    #[test]
    fn decodes_threads_with_their_comments() {
        let conversation = parse(SAMPLE);
        assert_eq!(conversation.threads.len(), 2);
        assert_eq!(conversation.unresolved_thread_count(), 1);

        let thread = conversation
            .thread(&ThreadId("PRRT_1".into()))
            .expect("thread present");
        assert_eq!(thread.path, "src/main.rs");
        assert_eq!(thread.line, Some(12));
        assert_eq!(thread.side, Side::Right);
        assert!(!thread.is_resolved);
        assert_eq!(thread.comments.len(), 2);
        // The REST id is what a reply has to be addressed to.
        assert_eq!(thread.comments[0].database_id, Some(900_001));
        assert_eq!(thread.comments[0].id, CommentId("PRRC_1".into()));

        let resolved = conversation
            .thread(&ThreadId("PRRT_2".into()))
            .expect("thread present");
        assert!(resolved.is_resolved);
        assert_eq!(resolved.side, Side::Left);
        assert_eq!(conversation.threads_for_path("src/lib.rs").count(), 1);
    }

    /// A thread belongs to the review that wrote its *first* comment; later
    /// replies belong to other reviews and must not re-file the thread.
    #[test]
    fn threads_attach_to_the_review_that_opened_them() {
        let conversation = parse(SAMPLE);
        let mut by_review = conversation.items.iter().filter_map(|item| match item {
            TimelineItem::Review { id, thread_ids, .. } => {
                Some((id.0.as_str(), thread_ids.clone()))
            }
            _ => None,
        });

        assert_eq!(
            by_review.next(),
            Some((
                "PRR_1",
                vec![ThreadId("PRRT_1".into()), ThreadId("PRRT_2".into())]
            ))
        );
        assert_eq!(by_review.next(), Some(("PRR_2", vec![])));
    }

    #[test]
    fn decodes_both_kinds_of_check() {
        let conversation = parse(SAMPLE);
        assert_eq!(
            conversation.checks,
            [
                CheckRun {
                    name: "build".into(),
                    state: Some(CheckState::Success),
                    url: Some("https://example.invalid/build".into()),
                },
                CheckRun {
                    name: "test".into(),
                    state: Some(CheckState::Pending),
                    url: Some("https://example.invalid/test".into()),
                },
                CheckRun {
                    name: "ci/legacy".into(),
                    state: Some(CheckState::Failure),
                    url: Some("https://example.invalid/legacy".into()),
                },
            ]
        );
    }

    #[test]
    fn check_conclusions_collapse_onto_the_status_scale() {
        for (conclusion, expected) in [
            ("SUCCESS", Some(CheckState::Success)),
            ("NEUTRAL", Some(CheckState::Success)),
            ("SKIPPED", Some(CheckState::Success)),
            ("FAILURE", Some(CheckState::Failure)),
            ("TIMED_OUT", Some(CheckState::Failure)),
            ("STARTUP_FAILURE", Some(CheckState::Failure)),
            ("CANCELLED", Some(CheckState::Failure)),
            ("ACTION_REQUIRED", Some(CheckState::Error)),
            ("STALE", Some(CheckState::Error)),
            ("SOMETHING_NEW", None),
        ] {
            assert_eq!(
                check_run_state(Some(conclusion), Some("COMPLETED")),
                expected,
                "{conclusion}"
            );
        }

        for status in ["QUEUED", "IN_PROGRESS", "WAITING", "PENDING", "REQUESTED"] {
            assert_eq!(
                check_run_state(None, Some(status)),
                Some(CheckState::Pending),
                "{status}"
            );
        }
        assert_eq!(check_run_state(None, Some("COMPLETED")), None);
        assert_eq!(check_run_state(None, None), None);
    }

    /// Every connection can be null, authors can be null, and an outdated thread
    /// has no line in the current diff. None of it may fail the decode.
    const NULL_HEAVY: &str = r#"{
      "data": {
        "rateLimit": null,
        "repository": {
          "pullRequest": {
            "body": "",
            "createdAt": "2026-07-30T10:00:00Z",
            "author": null,
            "comments": { "nodes": null },
            "reviews": null,
            "reviewThreads": {
              "nodes": [
                null,
                {
                  "id": "PRRT_9",
                  "path": "src/gone.rs",
                  "line": null,
                  "originalLine": 88,
                  "diffSide": "RIGHT",
                  "isResolved": false,
                  "isOutdated": true,
                  "comments": {
                    "nodes": [
                      {
                        "id": "PRRC_9",
                        "databaseId": null,
                        "body": "stale note",
                        "createdAt": "2026-07-30T12:00:00Z",
                        "author": null,
                        "pullRequestReview": null
                      }
                    ]
                  }
                }
              ]
            },
            "timelineItems": null,
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": null } }] }
          }
        }
      }
    }"#;

    #[test]
    fn tolerates_nulls_everywhere() {
        let conversation = parse(NULL_HEAVY);

        assert_eq!(conversation.items.len(), 1);
        let TimelineItem::Body { author, body, .. } = &conversation.items[0] else {
            panic!("expected only the body");
        };
        assert!(author.is_none());
        assert!(body.is_empty());

        assert!(conversation.checks.is_empty());

        // The null entry in `nodes` is dropped, the real thread survives.
        assert_eq!(conversation.threads.len(), 1);
        let thread = &conversation.threads[0];
        assert!(thread.is_outdated);
        assert_eq!(thread.line, None);
        assert_eq!(thread.original_line, Some(88));
        assert_eq!(thread.comments.len(), 1);
        assert!(thread.comments[0].author.is_none());
        // No database id means no reply affordance, not a decode failure.
        assert_eq!(thread.comments[0].database_id, None);
    }

    #[test]
    fn a_missing_pull_request_decodes_as_none() {
        let body = r#"{
          "data": { "rateLimit": null, "repository": { "pullRequest": null } },
          "errors": [{ "type": "NOT_FOUND", "message": "Could not resolve to a PullRequest" }]
        }"#;
        let response: GraphQlResponse<ConversationQueryData> =
            serde_json::from_str(body).expect("should decode");
        assert_eq!(response.errors[0].kind.as_deref(), Some("NOT_FOUND"));
        assert!(
            response
                .data
                .expect("data key present")
                .repository
                .expect("repository key present")
                .pull_request
                .is_none()
        );
    }

    #[test]
    fn events_without_a_timestamp_are_dropped() {
        let node: TimelineEventNode =
            serde_json::from_str(r#"{ "__typename": "SomeFutureEvent" }"#).expect("decodes");
        assert!(node.into_domain().is_none());
    }
}
