//! Canonical application state.

use chrono::{DateTime, Utc};

use crate::{
    feed::FeedFilter,
    model::{PrNumber, PullRequest, RepoId},
};

/// Per-repository fetch status.
///
/// `Failed` is deliberately separate from "has no PRs": a repo whose refresh
/// failed may still hold usable stale data, and the UI must be able to tell the
/// difference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Configured but never fetched.
    Idle,
    /// A fetch is in flight.
    Loading,
    Loaded {
        at: DateTime<Utc>,
    },
    Failed {
        message: String,
        at: DateTime<Utc>,
    },
}

impl LoadState {
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Failed { message, .. } => Some(message),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RepoState {
    pub id: RepoId,
    pub prs: Vec<PullRequest>,
    pub load: LoadState,
    pub collapsed: bool,
}

impl RepoState {
    pub fn new(id: RepoId) -> Self {
        Self {
            id,
            prs: Vec::new(),
            load: LoadState::Idle,
            collapsed: false,
        }
    }
}

/// Selection is stored by identity, never by feed index — indices are
/// positional and invalidated by every refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub repo: RepoId,
    pub pr: PrNumber,
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub repos: Vec<RepoState>,
    pub filter: FeedFilter,
    pub selection: Option<Selection>,
}

impl AppState {
    pub fn with_repos(ids: impl IntoIterator<Item = RepoId>) -> Self {
        Self {
            repos: ids.into_iter().map(RepoState::new).collect(),
            ..Default::default()
        }
    }

    pub fn repo(&self, id: &RepoId) -> Option<&RepoState> {
        self.repos.iter().find(|r| &r.id == id)
    }

    pub fn repo_mut(&mut self, id: &RepoId) -> Option<&mut RepoState> {
        self.repos.iter_mut().find(|r| &r.id == id)
    }

    /// Resolve the current selection to a live pull request, if it still exists.
    /// Returns `None` when the PR was merged or closed out from under us.
    pub fn selected_pr(&self) -> Option<(&RepoState, &PullRequest)> {
        let selection = self.selection.as_ref()?;
        let repo = self.repo(&selection.repo)?;
        let pr = repo.prs.iter().find(|p| p.number == selection.pr)?;
        Some((repo, pr))
    }

    pub fn total_open_prs(&self) -> usize {
        self.repos.iter().map(|r| r.prs.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mergeable, PrNumber};

    fn repo_with(id: &str, numbers: &[u32]) -> RepoState {
        let mut state = RepoState::new(id.parse().expect("valid repo id"));
        state.prs = numbers
            .iter()
            .map(|n| PullRequest {
                number: PrNumber(*n),
                title: format!("PR {n}"),
                url: String::new(),
                is_draft: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                author: None,
                head_ref: "feature".into(),
                head_sha: "abc123".into(),
                base_ref: "main".into(),
                additions: 0,
                deletions: 0,
                changed_files: 0,
                mergeable: Mergeable::Unknown,
                review_decision: None,
                labels: Vec::new(),
                comment_count: 0,
                checks: None,
            })
            .collect();
        state.load = LoadState::Loaded { at: Utc::now() };
        state
    }

    #[test]
    fn resolves_selection_to_live_pr() {
        let state = AppState {
            repos: vec![repo_with("a/b", &[1, 2])],
            selection: Some(Selection {
                repo: "a/b".parse().expect("valid repo id"),
                pr: PrNumber(2),
            }),
            ..Default::default()
        };

        let (repo, pr) = state.selected_pr().expect("selection should resolve");
        assert_eq!(repo.id.to_string(), "a/b");
        assert_eq!(pr.number, PrNumber(2));
    }

    #[test]
    fn selection_of_vanished_pr_resolves_to_none() {
        let state = AppState {
            repos: vec![repo_with("a/b", &[1])],
            selection: Some(Selection {
                repo: "a/b".parse().expect("valid repo id"),
                pr: PrNumber(99),
            }),
            ..Default::default()
        };
        assert!(state.selected_pr().is_none());
    }

    #[test]
    fn counts_open_prs_across_repos() {
        let state = AppState {
            repos: vec![repo_with("a/b", &[1, 2]), repo_with("c/d", &[3])],
            ..Default::default()
        };
        assert_eq!(state.total_open_prs(), 3);
    }
}
