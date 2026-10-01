//! Canonical application state.

use chrono::{DateTime, Utc};

use crate::{
    feed::FeedFilter,
    issue::{Issue, IssueNumber},
    model::{Divergence, PrNumber, PullRequest, RepoId},
    stack::Stack,
    repo_meta::RepoMeta,
    tabs::FeedTab,
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
    /// Fetch status of `prs`.
    pub load: LoadState,
    /// Open issues, most recently updated first.
    pub issues: Vec<Issue>,
    /// Fetch status of `issues`, separate from `load` because the two lists
    /// are separate requests: one can fail while the other succeeds, and each
    /// tab must report its own.
    pub issues_load: LoadState,
    /// Collapsing is per repository, not per tab: it hides the repository's
    /// body on both.
    pub collapsed: bool,
    /// Stacks GitHub knows about in this repository, from its Stacks API (or
    /// the cache of it). Detected chains are not stored: they are derived
    /// from `prs` every time the feed is built.
    pub stacks: Vec<Stack>,
    /// The repository's own facts — owner, push and creation times, stars —
    /// for sorting repositories. `None` until the first refresh or cache
    /// read supplies them; the sort places unknown values last.
    pub meta: Option<RepoMeta>,
}

impl RepoState {
    pub fn new(id: RepoId) -> Self {
        Self {
            id,
            prs: Vec::new(),
            load: LoadState::Idle,
            issues: Vec::new(),
            issues_load: LoadState::Idle,
            collapsed: false,
            stacks: Vec::new(),
            meta: None,
        }
    }
}

/// What the detail pane is showing.
///
/// Stored by identity, never by feed index — indices are positional and
/// invalidated by every refresh. The kind is part of the identity: a pull
/// request and an issue never share a number within a repository, but which
/// pane opens, and which endpoints it talks to, depends on knowing which one
/// was picked rather than looking the number up in both lists.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Selection {
    PullRequest { repo: RepoId, number: PrNumber },
    Issue { repo: RepoId, number: IssueNumber },
}

impl Selection {
    pub fn repo(&self) -> &RepoId {
        match self {
            Self::PullRequest { repo, .. } | Self::Issue { repo, .. } => repo,
        }
    }

    /// The feed tab this selection lives on.
    pub fn tab(&self) -> FeedTab {
        match self {
            Self::PullRequest { .. } => FeedTab::PullRequests,
            Self::Issue { .. } => FeedTab::Issues,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub repos: Vec<RepoState>,
    pub filter: FeedFilter,
    /// Which list the feed shows. Persisted, like the filter's preferences.
    pub tab: FeedTab,
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
        let Some(Selection::PullRequest { repo, number }) = self.selection.as_ref() else {
            return None;
        };
        let repo = self.repo(repo)?;
        let pr = repo.prs.iter().find(|p| p.number == *number)?;
        Some((repo, pr))
    }

    /// The issue counterpart of [`AppState::selected_pr`]: `None` when nothing
    /// is selected, a pull request is, or the issue has left the open list.
    pub fn selected_issue(&self) -> Option<(&RepoState, &Issue)> {
        let Some(Selection::Issue { repo, number }) = self.selection.as_ref() else {
            return None;
        };
        let repo = self.repo(repo)?;
        let issue = repo.issues.iter().find(|i| i.number == *number)?;
        Some((repo, issue))
    }

    pub fn total_open_prs(&self) -> usize {
        self.repos.iter().map(|r| r.prs.len()).sum()
    }

    pub fn total_open_issues(&self) -> usize {
        self.repos.iter().map(|r| r.issues.len()).sum()
    }
}

/// Copy each pull request's `base_divergence` from `old` onto the entry in
/// `new` with the same number.
///
/// A refresh replaces a repository's list wholesale, and the divergence
/// counts arrive from a separate query issued after it. Without this the row
/// chip would blink out on every poll and reappear a round trip later; with it
/// the previous answer stands until the new one lands. Matched by number, not
/// position, because the feed is ordered by update time and a refresh is
/// exactly when that order changes.
///
/// Only a known value is carried: a `None` in `new` stays `None` when the
/// old list had nothing for that number, and a `Some` already in `new` is
/// left alone.
pub fn carry_forward_divergence(old: &[PullRequest], new: &mut [PullRequest]) {
    for pr in new.iter_mut().filter(|pr| pr.base_divergence.is_none()) {
        pr.base_divergence = old
            .iter()
            .find(|previous| previous.number == pr.number)
            .and_then(|previous| previous.base_divergence);
    }
}

/// The numbers and `(base, head)` ref pairs of a repository's pull requests,
/// in the shape `GitHubClient::divergences` takes and [`apply_divergences`]
/// matches the answers back by.
pub fn divergence_query(prs: &[PullRequest]) -> (Vec<PrNumber>, Vec<(String, String)>) {
    prs.iter()
        .map(|pr| (pr.number, (pr.base_ref.clone(), pr.head_ref.clone())))
        .unzip()
}

/// Write a batch of divergence answers onto the pull requests they were asked
/// about.
///
/// Matched by number rather than by position: a refresh may have landed while
/// the batch was in flight, reordering or replacing the list, and a count
/// written to the wrong row is worse than none. An unanswered entry (`None`,
/// the cross-fork case) leaves whatever the pull request already had.
pub fn apply_divergences(
    prs: &mut [PullRequest],
    numbers: &[PrNumber],
    divergences: Vec<Option<Divergence>>,
) {
    let answered = numbers
        .iter()
        .zip(divergences)
        .filter_map(|(number, divergence)| divergence.map(|d| (*number, d)));
    for (number, divergence) in answered {
        if let Some(pr) = prs.iter_mut().find(|pr| pr.number == number) {
            pr.base_divergence = Some(divergence);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MergeStateStatus, Mergeable, NodeId};

    fn repo_with(id: &str, numbers: &[u32]) -> RepoState {
        let mut state = RepoState::new(id.parse().expect("valid repo id"));
        state.prs = numbers
            .iter()
            .map(|n| PullRequest {
                number: PrNumber(*n),
                node_id: NodeId(format!("PR_{}", *n)),
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
                merge_state: MergeStateStatus::Unknown,
                review_decision: None,
                assignees: Vec::new(),
                review_requests: Vec::new(),
                labels: Vec::new(),
                comment_count: 0,
                checks: None,
                base_divergence: None,
                is_cross_repository: false,
                pushed_at: None,
            })
            .collect();
        state.load = LoadState::Loaded { at: Utc::now() };
        state
    }

    #[test]
    fn resolves_selection_to_live_pr() {
        let state = AppState {
            repos: vec![repo_with("a/b", &[1, 2])],
            selection: Some(Selection::PullRequest {
                repo: "a/b".parse().expect("valid repo id"),
                number: PrNumber(2),
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
            selection: Some(Selection::PullRequest {
                repo: "a/b".parse().expect("valid repo id"),
                number: PrNumber(99),
            }),
            ..Default::default()
        };
        assert!(state.selected_pr().is_none());
    }

    fn with_issues(id: &str, prs: &[u32], issues: &[u32]) -> RepoState {
        let mut state = repo_with(id, prs);
        state.issues = issues
            .iter()
            .copied()
            .map(crate::test_support::issue)
            .collect();
        state.issues_load = LoadState::Loaded { at: Utc::now() };
        state
    }

    #[test]
    fn resolves_an_issue_selection_to_the_live_issue() {
        let state = AppState {
            repos: vec![with_issues("a/b", &[1], &[5, 6])],
            selection: Some(Selection::Issue {
                repo: "a/b".parse().expect("valid repo id"),
                number: IssueNumber(6),
            }),
            ..Default::default()
        };
        let (repo, issue) = state.selected_issue().expect("selection should resolve");
        assert_eq!(repo.id.to_string(), "a/b");
        assert_eq!(issue.number, IssueNumber(6));
        assert!(state.selected_pr().is_none());
    }

    /// The kind is part of the identity: an issue selection never resolves to
    /// a pull request with the same number, nor the other way round.
    #[test]
    fn a_selection_resolves_only_against_its_own_kind() {
        let repos = vec![with_issues("a/b", &[3], &[3])];
        let issue = AppState {
            repos: repos.clone(),
            selection: Some(Selection::Issue {
                repo: "a/b".parse().expect("valid repo id"),
                number: IssueNumber(3),
            }),
            ..Default::default()
        };
        assert!(issue.selected_pr().is_none());
        assert!(issue.selected_issue().is_some());

        let pr = AppState {
            repos,
            selection: Some(Selection::PullRequest {
                repo: "a/b".parse().expect("valid repo id"),
                number: PrNumber(3),
            }),
            ..Default::default()
        };
        assert!(pr.selected_issue().is_none());
        assert!(pr.selected_pr().is_some());
    }

    #[test]
    fn a_closed_issue_resolves_to_none() {
        let state = AppState {
            repos: vec![with_issues("a/b", &[], &[1])],
            selection: Some(Selection::Issue {
                repo: "a/b".parse().expect("valid repo id"),
                number: IssueNumber(2),
            }),
            ..Default::default()
        };
        assert!(state.selected_issue().is_none());
    }

    #[test]
    fn a_selection_knows_its_repo_and_tab() {
        let repo: RepoId = "a/b".parse().expect("valid repo id");
        let pr = Selection::PullRequest {
            repo: repo.clone(),
            number: PrNumber(1),
        };
        let issue = Selection::Issue {
            repo: repo.clone(),
            number: IssueNumber(1),
        };
        assert_eq!(pr.repo(), &repo);
        assert_eq!(issue.repo(), &repo);
        assert_eq!(pr.tab(), FeedTab::PullRequests);
        assert_eq!(issue.tab(), FeedTab::Issues);
        assert_ne!(pr, issue);
    }

    #[test]
    fn counts_open_issues_across_repos() {
        let state = AppState {
            repos: vec![
                with_issues("a/b", &[], &[1, 2]),
                with_issues("c/d", &[], &[3]),
            ],
            ..Default::default()
        };
        assert_eq!(state.total_open_issues(), 3);
    }

    /// The carry matches by number so a reordered refresh keeps each count
    /// on its own pull request, and a fresh answer is never overwritten by
    /// a stale one.
    #[test]
    fn carries_divergence_forward_by_number_without_clobbering_fresh_values() {
        let mut old = repo_with("a/b", &[1, 2, 3]).prs;
        old[0].base_divergence = Some(Divergence::new(0, 4));
        old[1].base_divergence = None;
        old[2].base_divergence = Some(Divergence::new(2, 0));

        // Reordered, one gone (#3), one new (#9), and #2 already answered.
        let mut new = repo_with("a/b", &[2, 9, 1]).prs;
        new[0].base_divergence = Some(Divergence::new(1, 1));

        carry_forward_divergence(&old, &mut new);

        assert_eq!(new[0].number, PrNumber(2));
        assert_eq!(new[0].base_divergence, Some(Divergence::new(1, 1)));
        assert_eq!(new[1].number, PrNumber(9));
        assert_eq!(new[1].base_divergence, None);
        assert_eq!(new[2].number, PrNumber(1));
        assert_eq!(new[2].base_divergence, Some(Divergence::new(0, 4)));
    }

    #[test]
    fn divergence_query_pairs_base_with_head_per_number() {
        let mut prs = repo_with("a/b", &[4, 7]).prs;
        prs[1].head_ref = "topic".into();
        prs[1].base_ref = "develop".into();
        let (numbers, pairs) = divergence_query(&prs);
        assert_eq!(numbers, vec![PrNumber(4), PrNumber(7)]);
        assert_eq!(
            pairs,
            vec![
                ("main".to_string(), "feature".to_string()),
                ("develop".to_string(), "topic".to_string()),
            ]
        );
    }

    /// Answers land on the pull request they were asked about even when the
    /// list was reordered in between, and an unanswered entry keeps what the
    /// pull request already had.
    #[test]
    fn divergences_apply_by_number_and_skip_unanswered_entries() {
        let mut prs = repo_with("a/b", &[1, 2, 3]).prs;
        prs[2].base_divergence = Some(Divergence::new(9, 9));
        // Asked about 3, 1, 2 — in that order — and 3 went unanswered.
        apply_divergences(
            &mut prs,
            &[PrNumber(3), PrNumber(1), PrNumber(2), PrNumber(99)],
            vec![
                None,
                Some(Divergence::new(1, 0)),
                Some(Divergence::new(0, 5)),
                Some(Divergence::new(4, 4)),
            ],
        );
        assert_eq!(prs[0].base_divergence, Some(Divergence::new(1, 0)));
        assert_eq!(prs[1].base_divergence, Some(Divergence::new(0, 5)));
        assert_eq!(prs[2].base_divergence, Some(Divergence::new(9, 9)));
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
