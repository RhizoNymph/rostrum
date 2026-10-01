//! The order the repository view lists its two halves in.
//!
//! Both halves follow the feed's **item** sort — the one the Sort popover
//! sets for pull requests and issues — so a repository reads the same in
//! its own view as in the feed. Unlike the feed, nothing is filtered out:
//! the view is the whole repository. Pure, so it is tested without a window.

use rostrum_core::{
    IssueIx, ItemSortKey, PrIx, RepoState, Selection, Sort,
    sort::{order_issues, order_items},
};

use super::nav::Position;

/// Display order of each list, as positions into `RepoState::prs` and
/// `RepoState::issues`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListOrder {
    pulls: Vec<PrIx>,
    issues: Vec<IssueIx>,
}

impl ListOrder {
    /// Every open pull request and issue of `repo`, sorted by `sort`. A
    /// repository that is gone has empty lists.
    pub fn new(repo: Option<&RepoState>, sort: Sort<ItemSortKey>) -> Self {
        let Some(repo) = repo else {
            return Self::default();
        };
        let mut pulls: Vec<PrIx> = (0..repo.prs.len()).map(PrIx).collect();
        order_items(&repo.prs, &mut pulls, sort);
        let mut issues: Vec<IssueIx> = (0..repo.issues.len()).map(IssueIx).collect();
        order_issues(&repo.issues, &mut issues, sort);
        Self { pulls, issues }
    }

    pub fn pulls(&self) -> usize {
        self.pulls.len()
    }

    pub fn issues(&self) -> usize {
        self.issues.len()
    }

    /// The pull request shown `row`th in the top list.
    pub fn pull_at(&self, row: usize) -> Option<PrIx> {
        self.pulls.get(row).copied()
    }

    /// The issue shown `row`th in the bottom list.
    pub fn issue_at(&self, row: usize) -> Option<IssueIx> {
        self.issues.get(row).copied()
    }

    /// Where `selection` is displayed, if it is an item of `repo`.
    pub fn position_of(&self, repo: &RepoState, selection: &Selection) -> Option<Position> {
        if selection.repo() != &repo.id {
            return None;
        }
        match selection {
            Selection::PullRequest { number, .. } => self
                .pulls
                .iter()
                .position(|ix| repo.prs.get(ix.0).is_some_and(|pr| pr.number == *number))
                .map(Position::pull),
            Selection::Issue { number, .. } => self
                .issues
                .iter()
                .position(|ix| {
                    repo.issues
                        .get(ix.0)
                        .is_some_and(|issue| issue.number == *number)
                })
                .map(Position::issue),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use rostrum_core::{
        Issue, IssueNumber, IssueState, LoadState, MergeStateStatus, Mergeable, NodeId, PrNumber,
        PullRequest, RepoId, SortDirection,
    };

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).expect("valid timestamp")
    }

    fn pr(number: u32, created: i64, updated: i64) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            node_id: NodeId(format!("PR_{number}")),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: false,
            created_at: at(created),
            updated_at: at(updated),
            author: None,
            head_ref: "feature".into(),
            head_sha: "deadbeef".into(),
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
            pushed_at: None,
        }
    }

    fn issue(number: u32, created: i64, updated: i64) -> Issue {
        Issue {
            number: IssueNumber(number),
            node_id: NodeId(format!("I_{number}")),
            title: format!("Issue {number}"),
            url: String::new(),
            state: IssueState::Open,
            created_at: at(created),
            updated_at: at(updated),
            author: None,
            assignees: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            milestone: None,
        }
    }

    fn repo() -> RepoState {
        let mut state = RepoState::new("o/r".parse::<RepoId>().expect("valid repo id"));
        state.prs = vec![pr(1, 10, 30), pr(2, 30, 10), pr(3, 20, 20)];
        state.load = LoadState::Loaded { at: at(0) };
        state.issues = vec![issue(7, 5, 50), issue(8, 15, 40), issue(9, 25, 60)];
        state.issues_load = LoadState::Loaded { at: at(0) };
        state
    }

    fn numbers(order: &ListOrder, repo: &RepoState) -> (Vec<u32>, Vec<u32>) {
        (
            (0..order.pulls())
                .filter_map(|row| order.pull_at(row))
                .map(|ix| repo.prs[ix.0].number.0)
                .collect(),
            (0..order.issues())
                .filter_map(|row| order.issue_at(row))
                .map(|ix| repo.issues[ix.0].number.0)
                .collect(),
        )
    }

    #[test]
    fn both_halves_follow_the_item_sort() {
        let repo = repo();
        let newest = ListOrder::new(Some(&repo), Sort::new(ItemSortKey::Created));
        assert_eq!(numbers(&newest, &repo), (vec![2, 3, 1], vec![9, 8, 7]));

        let oldest_update = ListOrder::new(
            Some(&repo),
            Sort::with_direction(ItemSortKey::Updated, SortDirection::Ascending),
        );
        assert_eq!(
            numbers(&oldest_update, &repo),
            (vec![2, 3, 1], vec![8, 7, 9])
        );
    }

    /// Issues have no branch: "pushed" sorts them by their last update.
    #[test]
    fn pushed_orders_issues_by_update() {
        let repo = repo();
        let order = ListOrder::new(Some(&repo), Sort::new(ItemSortKey::Pushed));
        assert_eq!(numbers(&order, &repo).1, vec![9, 7, 8]);
    }

    #[test]
    fn a_selection_is_found_at_its_displayed_row() {
        let repo = repo();
        let order = ListOrder::new(Some(&repo), Sort::new(ItemSortKey::Created));
        let pull = Selection::PullRequest {
            repo: repo.id.clone(),
            number: PrNumber(1),
        };
        let issue = Selection::Issue {
            repo: repo.id.clone(),
            number: IssueNumber(9),
        };
        assert_eq!(order.position_of(&repo, &pull), Some(Position::pull(2)));
        assert_eq!(order.position_of(&repo, &issue), Some(Position::issue(0)));
    }

    #[test]
    fn a_selection_elsewhere_or_gone_has_no_row() {
        let repo = repo();
        let order = ListOrder::new(Some(&repo), Sort::new(ItemSortKey::Created));
        let other_repo = Selection::PullRequest {
            repo: "o/other".parse().expect("valid repo id"),
            number: PrNumber(1),
        };
        let closed = Selection::Issue {
            repo: repo.id.clone(),
            number: IssueNumber(42),
        };
        // Same number, other kind: an issue selection never lands on a pull
        // request.
        let wrong_kind = Selection::Issue {
            repo: repo.id.clone(),
            number: IssueNumber(1),
        };
        assert_eq!(order.position_of(&repo, &other_repo), None);
        assert_eq!(order.position_of(&repo, &closed), None);
        assert_eq!(order.position_of(&repo, &wrong_kind), None);
    }

    #[test]
    fn a_missing_repository_lists_nothing() {
        let order = ListOrder::new(None, Sort::new(ItemSortKey::Created));
        assert_eq!((order.pulls(), order.issues()), (0, 0));
        assert_eq!(order.pull_at(0), None);
    }
}
