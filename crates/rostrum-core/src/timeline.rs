//! The conversation model for a single pull request.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{CheckState, Side, User};

/// GraphQL node id of an issue or review comment.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommentId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    Pending,
    Commented,
    Approved,
    ChangesRequested,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadComment {
    pub id: CommentId,
    /// REST id, needed to reply into a thread (the GraphQL node id will not do).
    pub database_id: Option<u64>,
    pub author: Option<User>,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// An inline review thread anchored to a line of the diff.
///
/// Stored once and referenced by id from both the conversation timeline and the
/// diff view, so resolving it in one place updates the other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub path: String,
    /// Line in the current diff; `None` once the thread is outdated.
    pub line: Option<u32>,
    /// Line the thread was originally written against.
    pub original_line: Option<u32>,
    pub side: Side,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub comments: Vec<ThreadComment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Merged,
    Closed,
    Reopened,
    ReadyForReview,
    ConvertedToDraft,
    HeadRefForcePushed,
    ReviewRequested { reviewer: String },
    Assigned { assignee: String },
    Labeled { name: String },
    Unlabeled { name: String },
    Renamed { from: String, to: String },
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TimelineItem {
    /// The pull request description itself; always first.
    Body {
        author: Option<User>,
        body: String,
        created_at: DateTime<Utc>,
    },
    Comment {
        id: CommentId,
        author: Option<User>,
        body: String,
        created_at: DateTime<Utc>,
    },
    Review {
        id: ReviewId,
        author: Option<User>,
        state: ReviewState,
        body: String,
        created_at: DateTime<Utc>,
        thread_ids: Vec<ThreadId>,
    },
    Event {
        kind: EventKind,
        actor: Option<User>,
        created_at: DateTime<Utc>,
    },
}

impl TimelineItem {
    pub fn created_at(&self) -> DateTime<Utc> {
        match self {
            Self::Body { created_at, .. }
            | Self::Comment { created_at, .. }
            | Self::Review { created_at, .. }
            | Self::Event { created_at, .. } => *created_at,
        }
    }

    pub fn author(&self) -> Option<&User> {
        match self {
            Self::Body { author, .. }
            | Self::Comment { author, .. }
            | Self::Review { author, .. } => author.as_ref(),
            Self::Event { actor, .. } => actor.as_ref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub state: Option<CheckState>,
    pub url: Option<String>,
}

/// Everything the detail pane needs for one pull request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    /// Sorted ascending by `created_at`, body first.
    pub items: Vec<TimelineItem>,
    /// Threads stored once; timeline reviews reference them by id.
    pub threads: Vec<ReviewThread>,
    pub checks: Vec<CheckRun>,
}

impl Conversation {
    pub fn thread(&self, id: &ThreadId) -> Option<&ReviewThread> {
        self.threads.iter().find(|thread| &thread.id == id)
    }

    /// Threads anchored to a file, in line order. Used by the diff view.
    pub fn threads_for_path<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a ReviewThread> {
        self.threads.iter().filter(move |t| t.path == path)
    }

    /// Sort items chronologically, keeping the body pinned first.
    pub fn sort(&mut self) {
        self.items.sort_by_key(|item| {
            (
                !matches!(item, TimelineItem::Body { .. }),
                item.created_at(),
            )
        });
    }

    pub fn unresolved_thread_count(&self) -> usize {
        self.threads.iter().filter(|t| !t.is_resolved).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid timestamp")
    }

    fn comment(id: &str, secs: i64) -> TimelineItem {
        TimelineItem::Comment {
            id: CommentId(id.into()),
            author: None,
            body: String::new(),
            created_at: at(secs),
        }
    }

    #[test]
    fn sort_puts_body_first_then_chronological() {
        let mut conversation = Conversation {
            items: vec![
                comment("c", 300),
                comment("a", 100),
                TimelineItem::Body {
                    author: None,
                    body: "desc".into(),
                    created_at: at(999),
                },
                comment("b", 200),
            ],
            ..Default::default()
        };
        conversation.sort();

        assert!(matches!(conversation.items[0], TimelineItem::Body { .. }));
        let ids: Vec<_> = conversation.items[1..]
            .iter()
            .map(|item| match item {
                TimelineItem::Comment { id, .. } => id.0.clone(),
                _ => unreachable!("only comments follow"),
            })
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn threads_are_looked_up_by_id_and_path() {
        let thread = ReviewThread {
            id: ThreadId("t1".into()),
            path: "src/main.rs".into(),
            line: Some(10),
            original_line: Some(10),
            side: Side::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![],
        };
        let conversation = Conversation {
            threads: vec![thread.clone()],
            ..Default::default()
        };

        assert_eq!(conversation.thread(&ThreadId("t1".into())), Some(&thread));
        assert!(conversation.thread(&ThreadId("nope".into())).is_none());
        assert_eq!(conversation.threads_for_path("src/main.rs").count(), 1);
        assert_eq!(conversation.threads_for_path("other.rs").count(), 0);
        assert_eq!(conversation.unresolved_thread_count(), 1);
    }
}
