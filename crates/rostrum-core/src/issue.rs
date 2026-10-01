//! Domain types describing issues.
//!
//! Issues and pull requests share one number space per repository, and
//! GitHub's issues REST endpoints accept either. They are still kept as
//! distinct types here: an issue has no branches, no diff and no merge state,
//! and a selection or a cache key that confused the two would open the wrong
//! pane or show the wrong conversation.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    model::{Label, LoginKey, NodeId, User},
    timeline::Conversation,
};

/// An issue number, unique within its repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IssueNumber(pub u32);

impl fmt::Display for IssueNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Why a closed issue was closed, as GitHub's `IssueClosedStateReason`
/// reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CloseReason {
    Completed,
    NotPlanned,
    Duplicate,
}

impl CloseReason {
    /// How the reason reads after "closed as".
    pub fn describe(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not planned",
            Self::Duplicate => "a duplicate",
        }
    }
}

/// Where an issue is in its life.
///
/// A reason only exists for a closed issue, so it lives inside `Closed`
/// rather than beside the state: an open issue with a close reason is not
/// representable. The reason is optional because issues closed before GitHub
/// recorded reasons have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum IssueState {
    Open,
    Closed(Option<CloseReason>),
}

impl IssueState {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    /// Short label for a chip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed(Some(CloseReason::NotPlanned)) => "closed: not planned",
            Self::Closed(Some(CloseReason::Duplicate)) => "closed: duplicate",
            Self::Closed(_) => "closed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    pub title: String,
}

/// One issue, as the feed lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub number: IssueNumber,
    /// GraphQL node id, carried for the same reason pull requests carry one:
    /// any GraphQL-only operation can address the issue without a lookup.
    pub node_id: NodeId,
    pub title: String,
    pub url: String,
    pub state: IssueState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// `None` for deleted accounts and some bot actors.
    pub author: Option<User>,
    pub assignees: Vec<User>,
    pub labels: Vec<Label>,
    pub comment_count: u32,
    pub milestone: Option<Milestone>,
}

impl Issue {
    /// Whether `login` opened this issue.
    pub fn is_authored_by(&self, login: &LoginKey) -> bool {
        self.author
            .as_ref()
            .is_some_and(|author| &author.key() == login)
    }

    /// Whether `login` is on the hook for this issue: they opened it or it is
    /// assigned to them.
    ///
    /// Mentions are not counted. The feed query has no bounded way to ask who
    /// an issue mentions — it would mean reading every comment body — and an
    /// assignment is the signal that actually means "waiting on you".
    pub fn involves(&self, login: &LoginKey) -> bool {
        self.is_authored_by(login) || self.assignees.iter().any(|user| &user.key() == login)
    }

    /// Text used for feed filtering, mirroring
    /// [`crate::PullRequest::matches_query`] so both tabs search alike.
    pub fn matches_query(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        self.title.to_lowercase().contains(&needle)
            || self.number.0.to_string().contains(&needle)
            || self
                .author
                .as_ref()
                .is_some_and(|a| a.login.to_lowercase().contains(&needle))
            || self
                .labels
                .iter()
                .any(|l| l.name.to_lowercase().contains(&needle))
            || self
                .milestone
                .as_ref()
                .is_some_and(|m| m.title.to_lowercase().contains(&needle))
    }
}

/// Everything the issue pane shows: the issue itself as of the detail fetch,
/// and its timeline.
///
/// The issue is carried here rather than read from the feed so the pane
/// outlives the issue leaving the feed — closing an issue drops it from the
/// open list on the next poll, and the pane must still be able to reopen it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IssueDetail {
    pub issue: Issue,
    /// Body first, then comments and events in chronological order. Issues
    /// have no review threads or checks, so those stay empty.
    pub conversation: Conversation,
}

/// A title GitHub will accept for a new issue: not blank, trimmed.
///
/// GitHub answers a blank title with a 422. Validating at construction means
/// the create form cannot even build a request that would be refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct IssueTitle(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("an issue needs a title")]
pub struct EmptyTitle;

impl IssueTitle {
    pub fn new(raw: &str) -> Result<Self, EmptyTitle> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(EmptyTitle);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::issue;

    fn user(login: &str) -> User {
        User {
            login: login.into(),
            avatar_url: None,
        }
    }

    #[test]
    fn involvement_is_author_or_assignee() {
        let issue = Issue {
            author: Some(user("Author")),
            assignees: vec![user("Assignee")],
            ..issue(1)
        };
        assert!(issue.is_authored_by(&LoginKey::new("author")));
        assert!(!issue.is_authored_by(&LoginKey::new("assignee")));
        assert!(issue.involves(&LoginKey::new("ASSIGNEE")));
        assert!(issue.involves(&LoginKey::new("author")));
        assert!(!issue.involves(&LoginKey::new("bystander")));
    }

    #[test]
    fn an_issue_without_an_author_is_authored_by_nobody() {
        let issue = issue(1);
        assert!(!issue.is_authored_by(&LoginKey::new("")));
        assert!(!issue.involves(&LoginKey::new("anyone")));
    }

    #[test]
    fn the_query_matches_title_number_author_labels_and_milestone() {
        let issue = Issue {
            title: "Crash on Startup".into(),
            author: Some(user("RhizoNymph")),
            labels: vec![Label {
                name: "C-bug".into(),
                color: "f00".into(),
            }],
            milestone: Some(Milestone {
                title: "1.101.0".into(),
            }),
            ..issue(4242)
        };
        for needle in ["", "startup", "4242", "rhizo", "c-BUG", "1.101"] {
            assert!(issue.matches_query(needle), "{needle}");
        }
        assert!(!issue.matches_query("absent"));
    }

    #[test]
    fn a_blank_title_is_refused_and_a_real_one_is_trimmed() {
        assert_eq!(IssueTitle::new(""), Err(EmptyTitle));
        assert_eq!(IssueTitle::new("  \n\t"), Err(EmptyTitle));
        let title = IssueTitle::new("  Fix it  ").expect("valid");
        assert_eq!(title.as_str(), "Fix it");
        assert_eq!(
            serde_json::to_value(&title).expect("serialises"),
            serde_json::json!("Fix it")
        );
    }

    /// The state is cached as JSON, so its encoding has to round-trip, and a
    /// reason can only ever be found inside `closed`.
    #[test]
    fn the_state_round_trips_with_its_reason_inside_closed() {
        for state in [
            IssueState::Open,
            IssueState::Closed(None),
            IssueState::Closed(Some(CloseReason::Completed)),
            IssueState::Closed(Some(CloseReason::NotPlanned)),
            IssueState::Closed(Some(CloseReason::Duplicate)),
        ] {
            let json = serde_json::to_string(&state).expect("encodes");
            let back: IssueState = serde_json::from_str(&json).expect("decodes");
            assert_eq!(back, state, "{json}");
        }
        assert!(IssueState::Open.is_open());
        assert!(!IssueState::Closed(None).is_open());
    }

    #[test]
    fn state_labels_name_the_reason_when_it_matters() {
        assert_eq!(IssueState::Open.label(), "open");
        assert_eq!(
            IssueState::Closed(Some(CloseReason::Completed)).label(),
            "closed"
        );
        assert_eq!(
            IssueState::Closed(Some(CloseReason::NotPlanned)).label(),
            "closed: not planned"
        );
        assert_eq!(IssueState::Closed(None).label(), "closed");
    }

    #[test]
    fn issue_numbers_display_like_github() {
        assert_eq!(IssueNumber(12).to_string(), "#12");
    }
}
