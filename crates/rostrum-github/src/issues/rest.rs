//! REST requests that change an issue, and the one that creates one.
//!
//! Each mutation is a value that knows its own method, path and body
//! ([`IssueMutation::call`]), so what goes over the wire is a pure function
//! the tests pin exactly, and the client has one place that sends any of
//! them.

use reqwest::Method;
use rostrum_core::{IssueNumber, IssueTitle, RepoId, User};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{client::encode_path_segment, rest::AddLabels};

/// How a close is recorded. The two reasons a person picks between; GitHub's
/// third, `duplicate`, is set by marking a duplicate rather than by closing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAs {
    Completed,
    NotPlanned,
}

impl CloseAs {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not_planned",
        }
    }
}

/// Opening or closing an issue, as one end state.
///
/// An end state rather than a toggle, as with draft conversion: a poll
/// landing between render and click can make the request redundant, never
/// make it do the opposite of what the button said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueStateChange {
    Close(CloseAs),
    Reopen,
}

/// A comment body GitHub will accept: not blank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentBody(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a comment needs some text")]
pub struct EmptyComment;

impl CommentBody {
    /// Trailing whitespace is kept: markdown can mean something by it, and
    /// the only rule GitHub enforces is "not blank".
    pub fn new(text: impl Into<String>) -> Result<Self, EmptyComment> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(EmptyComment);
        }
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Body of the two `/assignees` endpoints, which take the same shape to add
/// (`POST`) and to remove (`DELETE`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Assignees {
    pub assignees: Vec<String>,
}

impl Assignees {
    pub fn new(logins: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            assignees: logins.into_iter().map(Into::into).collect(),
        }
    }
}

/// A new title and description for an existing issue.
///
/// Both are always sent: the editor edits them together, and sending an
/// empty body is how a description is cleared. The title cannot be blank.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IssueEdit {
    pub title: IssueTitle,
    pub body: String,
}

/// Every change the issue pane can make to an existing issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueMutation {
    Edit(IssueEdit),
    Comment(CommentBody),
    SetState(IssueStateChange),
    AddLabels(AddLabels),
    RemoveLabel(String),
    AddAssignees(Assignees),
    RemoveAssignees(Assignees),
}

/// One REST request, fully described: method, path below the API root, and
/// JSON body if any.
#[derive(Clone, Debug, PartialEq)]
pub struct RestCall {
    pub method: Method,
    pub path: String,
    pub body: Option<Value>,
}

impl IssueMutation {
    /// What the in-flight banner says.
    pub fn progress_label(&self) -> &'static str {
        match self {
            Self::Edit(_) => "Saving",
            Self::Comment(_) => "Commenting",
            Self::SetState(IssueStateChange::Close(CloseAs::Completed)) => "Closing as completed",
            Self::SetState(IssueStateChange::Close(CloseAs::NotPlanned)) => {
                "Closing as not planned"
            }
            Self::SetState(IssueStateChange::Reopen) => "Reopening",
            Self::AddLabels(_) => "Adding label",
            Self::RemoveLabel(_) => "Removing label",
            Self::AddAssignees(_) => "Assigning",
            Self::RemoveAssignees(_) => "Unassigning",
        }
    }

    /// Whether the request would change nothing. GitHub answers an empty
    /// label or assignee list with a 422, and "add nobody" already holds.
    pub fn is_noop(&self) -> bool {
        match self {
            Self::AddLabels(labels) => labels.is_empty(),
            Self::AddAssignees(people) | Self::RemoveAssignees(people) => {
                people.assignees.is_empty()
            }
            Self::Edit(_) | Self::Comment(_) | Self::SetState(_) | Self::RemoveLabel(_) => false,
        }
    }

    pub fn call(&self, repo: &RepoId, number: IssueNumber) -> RestCall {
        let issue = format!(
            "/repos/{}/{}/issues/{}",
            repo.owner(),
            repo.name(),
            number.0
        );
        let (method, path, body) = match self {
            Self::Edit(edit) => (Method::PATCH, issue, Some(to_value(edit))),
            Self::Comment(body) => (
                Method::POST,
                format!("{issue}/comments"),
                Some(json!({ "body": body.as_str() })),
            ),
            Self::SetState(IssueStateChange::Close(reason)) => (
                Method::PATCH,
                issue,
                Some(json!({ "state": "closed", "state_reason": reason.as_api_str() })),
            ),
            Self::SetState(IssueStateChange::Reopen) => (
                Method::PATCH,
                issue,
                Some(json!({ "state": "open", "state_reason": "reopened" })),
            ),
            Self::AddLabels(labels) => (
                Method::POST,
                format!("{issue}/labels"),
                Some(to_value(labels)),
            ),
            // The name is a path segment, so it is escaped; real labels hold
            // spaces, slashes and colons.
            Self::RemoveLabel(name) => (
                Method::DELETE,
                format!("{issue}/labels/{}", encode_path_segment(name)),
                None,
            ),
            Self::AddAssignees(people) => (
                Method::POST,
                format!("{issue}/assignees"),
                Some(to_value(people)),
            ),
            Self::RemoveAssignees(people) => (
                Method::DELETE,
                format!("{issue}/assignees"),
                Some(to_value(people)),
            ),
        };
        RestCall { method, path, body }
    }
}

/// Body of `POST /repos/{o}/{r}/issues`.
///
/// The title is an [`IssueTitle`], so a request with a blank one — which
/// GitHub refuses with a 422 — cannot be built. Empty optional parts are
/// omitted rather than sent as empty values, leaving GitHub's defaults alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CreateIssue {
    pub title: IssueTitle,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assignees: Vec<String>,
}

impl CreateIssue {
    pub fn new(title: IssueTitle) -> Self {
        Self {
            title,
            body: String::new(),
            labels: Vec::new(),
            assignees: Vec::new(),
        }
    }

    /// A blank body is omitted, so GitHub records no description rather than
    /// an empty one.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        let body = body.into();
        self.body = if body.trim().is_empty() {
            String::new()
        } else {
            body
        };
        self
    }

    pub fn with_labels(mut self, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_assignees(mut self, logins: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.assignees = logins.into_iter().map(Into::into).collect();
        self
    }

    pub fn call(&self, repo: &RepoId) -> RestCall {
        RestCall {
            method: Method::POST,
            path: format!("/repos/{}/{}/issues", repo.owner(), repo.name()),
            body: Some(to_value(self)),
        }
    }
}

/// The part of GitHub's created-issue response the app needs: where the new
/// issue lives.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CreatedIssue {
    pub number: u32,
    pub html_url: String,
}

/// One entry of `GET /repos/{o}/{r}/assignees`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AssignableUser {
    pub login: String,
    pub avatar_url: Option<String>,
}

impl From<AssignableUser> for User {
    fn from(user: AssignableUser) -> Self {
        User {
            login: user.login,
            avatar_url: user.avatar_url,
        }
    }
}

/// Serialising these plain structs cannot fail; a failure would be a bug in
/// a derive, so it degrades to `null` rather than threading an error through
/// every call site.
fn to_value(value: &impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoId {
        RepoId::new("rust-lang", "rust")
    }

    const N: IssueNumber = IssueNumber(42);

    fn call(mutation: IssueMutation) -> RestCall {
        mutation.call(&repo(), N)
    }

    #[test]
    fn a_comment_posts_its_body_to_the_comments_endpoint() {
        let body = CommentBody::new("Looks like a dupe of #7").expect("not blank");
        assert_eq!(
            call(IssueMutation::Comment(body)),
            RestCall {
                method: Method::POST,
                path: "/repos/rust-lang/rust/issues/42/comments".into(),
                body: Some(json!({ "body": "Looks like a dupe of #7" })),
            }
        );
    }

    #[test]
    fn an_edit_patches_title_and_body() {
        let edit = IssueEdit {
            title: IssueTitle::new(" New title ").expect("valid"),
            body: "New **body**".into(),
        };
        assert_eq!(
            call(IssueMutation::Edit(edit)),
            RestCall {
                method: Method::PATCH,
                path: "/repos/rust-lang/rust/issues/42".into(),
                body: Some(json!({ "title": "New title", "body": "New **body**" })),
            }
        );
    }

    /// Clearing the description is an edit like any other: the empty body is
    /// sent, not omitted.
    #[test]
    fn an_edit_that_clears_the_body_still_sends_it() {
        let edit = IssueEdit {
            title: IssueTitle::new("t").expect("valid"),
            body: String::new(),
        };
        assert_eq!(
            call(IssueMutation::Edit(edit)).body,
            Some(json!({ "title": "t", "body": "" }))
        );
    }

    #[test]
    fn a_blank_comment_cannot_be_built() {
        assert_eq!(CommentBody::new(""), Err(EmptyComment));
        assert_eq!(CommentBody::new(" \n "), Err(EmptyComment));
        assert_eq!(CommentBody::new("x  ").expect("valid").as_str(), "x  ");
    }

    #[test]
    fn closing_as_completed_patches_state_and_reason() {
        assert_eq!(
            call(IssueMutation::SetState(IssueStateChange::Close(
                CloseAs::Completed
            ))),
            RestCall {
                method: Method::PATCH,
                path: "/repos/rust-lang/rust/issues/42".into(),
                body: Some(json!({ "state": "closed", "state_reason": "completed" })),
            }
        );
    }

    #[test]
    fn closing_as_not_planned_sends_the_api_spelling() {
        assert_eq!(
            call(IssueMutation::SetState(IssueStateChange::Close(
                CloseAs::NotPlanned
            )))
            .body,
            Some(json!({ "state": "closed", "state_reason": "not_planned" }))
        );
    }

    #[test]
    fn reopening_patches_the_state_back_to_open() {
        assert_eq!(
            call(IssueMutation::SetState(IssueStateChange::Reopen)),
            RestCall {
                method: Method::PATCH,
                path: "/repos/rust-lang/rust/issues/42".into(),
                body: Some(json!({ "state": "open", "state_reason": "reopened" })),
            }
        );
    }

    #[test]
    fn adding_labels_posts_a_named_array() {
        assert_eq!(
            call(IssueMutation::AddLabels(AddLabels::new([
                "C-bug", "P-high"
            ]))),
            RestCall {
                method: Method::POST,
                path: "/repos/rust-lang/rust/issues/42/labels".into(),
                body: Some(json!({ "labels": ["C-bug", "P-high"] })),
            }
        );
    }

    #[test]
    fn removing_a_label_deletes_its_escaped_path_with_no_body() {
        assert_eq!(
            call(IssueMutation::RemoveLabel("status: needs triage".into())),
            RestCall {
                method: Method::DELETE,
                path: "/repos/rust-lang/rust/issues/42/labels/status%3A%20needs%20triage".into(),
                body: None,
            }
        );
    }

    #[test]
    fn assigning_posts_the_logins() {
        assert_eq!(
            call(IssueMutation::AddAssignees(Assignees::new([
                "alice", "bob"
            ]))),
            RestCall {
                method: Method::POST,
                path: "/repos/rust-lang/rust/issues/42/assignees".into(),
                body: Some(json!({ "assignees": ["alice", "bob"] })),
            }
        );
    }

    #[test]
    fn unassigning_deletes_with_the_same_body_shape() {
        assert_eq!(
            call(IssueMutation::RemoveAssignees(Assignees::new(["alice"]))),
            RestCall {
                method: Method::DELETE,
                path: "/repos/rust-lang/rust/issues/42/assignees".into(),
                body: Some(json!({ "assignees": ["alice"] })),
            }
        );
    }

    #[test]
    fn empty_label_and_assignee_lists_are_noops() {
        assert!(IssueMutation::AddLabels(AddLabels::new(Vec::<String>::new())).is_noop());
        assert!(IssueMutation::AddAssignees(Assignees::new(Vec::<String>::new())).is_noop());
        assert!(IssueMutation::RemoveAssignees(Assignees::new(Vec::<String>::new())).is_noop());
        assert!(!IssueMutation::AddAssignees(Assignees::new(["a"])).is_noop());
        assert!(!IssueMutation::RemoveLabel("x".into()).is_noop());
        assert!(!IssueMutation::SetState(IssueStateChange::Reopen).is_noop());
    }

    #[test]
    fn every_mutation_names_its_progress() {
        let labels = [
            IssueMutation::Edit(IssueEdit {
                title: IssueTitle::new("t").expect("valid"),
                body: String::new(),
            }),
            IssueMutation::Comment(CommentBody::new("x").expect("valid")),
            IssueMutation::SetState(IssueStateChange::Close(CloseAs::Completed)),
            IssueMutation::SetState(IssueStateChange::Close(CloseAs::NotPlanned)),
            IssueMutation::SetState(IssueStateChange::Reopen),
            IssueMutation::AddLabels(AddLabels::new(["a"])),
            IssueMutation::RemoveLabel("a".into()),
            IssueMutation::AddAssignees(Assignees::new(["a"])),
            IssueMutation::RemoveAssignees(Assignees::new(["a"])),
        ]
        .map(|mutation| mutation.progress_label());
        let distinct: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(distinct.len(), labels.len(), "{labels:?}");
    }

    #[test]
    fn creating_posts_title_body_labels_and_assignees() {
        let request = CreateIssue::new(IssueTitle::new("  Crash on start  ").expect("valid"))
            .with_body("Steps:\n1. open")
            .with_labels(["C-bug"])
            .with_assignees(["alice"]);
        assert_eq!(
            request.call(&repo()),
            RestCall {
                method: Method::POST,
                path: "/repos/rust-lang/rust/issues".into(),
                body: Some(json!({
                    "title": "Crash on start",
                    "body": "Steps:\n1. open",
                    "labels": ["C-bug"],
                    "assignees": ["alice"],
                })),
            }
        );
    }

    /// The bare minimum is a title; empty extras are left out so GitHub
    /// applies its own defaults rather than being told "no labels".
    #[test]
    fn creating_with_only_a_title_sends_only_the_title() {
        let request = CreateIssue::new(IssueTitle::new("Title").expect("valid")).with_body("  \n");
        assert_eq!(
            request.call(&repo()).body,
            Some(json!({ "title": "Title" }))
        );
    }

    #[test]
    fn decodes_a_captured_created_issue_response() {
        let created: CreatedIssue =
            serde_json::from_str(include_str!("../../fixtures/issues/rest_issue.json"))
                .expect("decodes");
        assert_eq!(created.number, 163598);
        assert_eq!(
            created.html_url,
            "https://github.com/rust-lang/rust/issues/163598"
        );
    }

    #[test]
    fn decodes_a_captured_assignee_page() {
        let page: Vec<AssignableUser> =
            serde_json::from_str(include_str!("../../fixtures/issues/rest_assignees.json"))
                .expect("decodes");
        assert_eq!(page.len(), 3);
        let user: User = page[0].clone().into();
        assert_eq!(user.login, "agu-z");
        assert!(user.avatar_url.is_some());
    }
}
