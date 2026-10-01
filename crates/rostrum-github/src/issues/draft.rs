//! The new-issue form's state, without the form.
//!
//! What the form holds and the rules for turning it into a request live here,
//! free of any UI toolkit, so they are tested directly and a second front end
//! can reuse them.

use std::collections::BTreeSet;

use rostrum_core::{EmptyTitle, IssueTitle, RepoId};

use crate::issues::rest::CreateIssue;

/// Why a draft cannot be sent yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DraftError {
    #[error("choose a repository for the issue")]
    NoRepository,
    #[error(transparent)]
    EmptyTitle(#[from] EmptyTitle),
}

/// An issue being written, not yet sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueDraft {
    repo: Option<RepoId>,
    /// Label names, by the repository's own spelling.
    labels: BTreeSet<String>,
    /// Logins to assign.
    assignees: BTreeSet<String>,
}

impl IssueDraft {
    pub fn new(repo: Option<RepoId>) -> Self {
        Self {
            repo,
            ..Default::default()
        }
    }

    pub fn repo(&self) -> Option<&RepoId> {
        self.repo.as_ref()
    }

    /// Move the draft to another repository.
    ///
    /// Labels and assignees are dropped: both are per repository, and a label
    /// picked from one palette that the other lacks would be silently created
    /// by GitHub, or refused outright for an assignee. Title and body stay
    /// with the form, which owns their text.
    pub fn set_repo(&mut self, repo: RepoId) {
        if self.repo.as_ref() == Some(&repo) {
            return;
        }
        self.repo = Some(repo);
        self.labels.clear();
        self.assignees.clear();
    }

    pub fn labels(&self) -> &BTreeSet<String> {
        &self.labels
    }

    pub fn assignees(&self) -> &BTreeSet<String> {
        &self.assignees
    }

    /// Add or remove a label, answering whether it is now applied.
    pub fn toggle_label(&mut self, name: &str) -> bool {
        toggle(&mut self.labels, name)
    }

    /// Add or remove an assignee, answering whether they are now assigned.
    pub fn toggle_assignee(&mut self, login: &str) -> bool {
        toggle(&mut self.assignees, login)
    }

    /// The repository and request this draft would send, or why it cannot be
    /// sent yet. The title is checked here, so a blank one never leaves the
    /// form.
    pub fn request(&self, title: &str, body: &str) -> Result<(RepoId, CreateIssue), DraftError> {
        let repo = self.repo.clone().ok_or(DraftError::NoRepository)?;
        let request = CreateIssue::new(IssueTitle::new(title)?)
            .with_body(body)
            .with_labels(self.labels.iter().cloned())
            .with_assignees(self.assignees.iter().cloned());
        Ok((repo, request))
    }
}

fn toggle(set: &mut BTreeSet<String>, value: &str) -> bool {
    if set.remove(value) {
        return false;
    }
    set.insert(value.to_string());
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn repo(name: &str) -> RepoId {
        name.parse().expect("valid repo id")
    }

    #[test]
    fn a_draft_needs_a_repository_and_a_title() {
        assert_eq!(
            IssueDraft::new(None).request("Title", "").map(|_| ()),
            Err(DraftError::NoRepository)
        );
        assert_eq!(
            IssueDraft::new(Some(repo("a/b")))
                .request("  ", "body")
                .map(|_| ()),
            Err(DraftError::EmptyTitle(EmptyTitle))
        );
    }

    #[test]
    fn a_complete_draft_becomes_the_request_body() {
        let mut draft = IssueDraft::new(Some(repo("a/b")));
        assert!(draft.toggle_label("C-bug"));
        assert!(draft.toggle_label("P-high"));
        assert!(!draft.toggle_label("P-high"));
        assert!(draft.toggle_assignee("alice"));

        let (target, request) = draft.request(" Crash ", "It crashes.").expect("valid");
        assert_eq!(target, repo("a/b"));
        assert_eq!(
            request.call(&target).body,
            Some(json!({
                "title": "Crash",
                "body": "It crashes.",
                "labels": ["C-bug"],
                "assignees": ["alice"],
            }))
        );
    }

    /// Labels and people belong to a repository; moving the draft must not
    /// carry them somewhere they do not exist.
    #[test]
    fn switching_repository_drops_labels_and_assignees() {
        let mut draft = IssueDraft::new(Some(repo("a/b")));
        draft.toggle_label("C-bug");
        draft.toggle_assignee("alice");

        draft.set_repo(repo("a/b"));
        assert_eq!(draft.labels().len(), 1, "same repository keeps the picks");

        draft.set_repo(repo("c/d"));
        assert_eq!(draft.repo(), Some(&repo("c/d")));
        assert!(draft.labels().is_empty());
        assert!(draft.assignees().is_empty());
    }
}
