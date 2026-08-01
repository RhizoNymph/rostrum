//! Keyboard navigation over the flattened feed.
//!
//! Pure functions over `&Feed`: no window, no store, no gpui. Feed indices are
//! positional and every refresh can invalidate them, so a keypress resolves the
//! store's identity-based [`Selection`] to a row index at the moment it fires
//! rather than caching one.

use rostrum_core::{Feed, FeedRow, PrIx, RepoIx, RepoState, Selection};

/// A navigation request from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Next,
    Previous,
    First,
    Last,
}

/// Locate the row the current selection occupies.
///
/// Returns `None` when nothing is selected, when the selected repository is
/// gone, when the selected pull request has closed, or when the active filter
/// or a collapsed repository has hidden its row.
pub fn selected_row(
    feed: &Feed,
    repos: &[RepoState],
    selection: Option<&Selection>,
) -> Option<usize> {
    let selection = selection?;
    let repo_ix = repos.iter().position(|repo| repo.id == selection.repo)?;
    let pr_ix = repos[repo_ix]
        .prs
        .iter()
        .position(|pr| pr.number == selection.pr)?;

    let target = FeedRow::PrRow {
        repo: RepoIx(repo_ix),
        pr: PrIx(pr_ix),
    };
    feed.rows().iter().position(|row| *row == target)
}

/// Resolve `nav` to the feed row it should land on, or `None` when the feed
/// holds no pull requests to land on at all.
///
/// Only `PrRow`s are navigable; headers, notices and spacers are skipped.
/// Movement deliberately does not wrap — `Next` at the last pull request and
/// `Previous` at the first stay put, so holding a key cannot silently teleport
/// the selection across the feed. With no live selection (`current` is `None`,
/// which includes a selection whose row has vanished) `Next` and `First` land
/// on the first pull request, `Previous` and `Last` on the last.
pub fn navigate(feed: &Feed, current: Option<usize>, nav: Nav) -> Option<usize> {
    let is_pr = |ix: &usize| matches!(feed.row(*ix), Some(FeedRow::PrRow { .. }));

    match (nav, current) {
        (Nav::First, _) | (Nav::Next, None) => (0..feed.len()).find(is_pr),
        (Nav::Last, _) | (Nav::Previous, None) => (0..feed.len()).rev().find(is_pr),
        (Nav::Next, Some(ix)) => ((ix + 1)..feed.len()).find(is_pr).or(Some(ix)),
        (Nav::Previous, Some(ix)) => (0..ix).rev().find(is_pr).or(Some(ix)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rostrum_core::{
        FeedFilter, LoadState, Mergeable, PrNumber, PullRequest, RepoId, RepoState, flatten,
    };

    fn pr(number: u32) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: "feature".into(),
            head_sha: "deadbeef".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            review_decision: None,
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
        }
    }

    fn repo(name: &str, numbers: &[u32]) -> RepoState {
        RepoState {
            id: name.parse::<RepoId>().expect("valid repo id"),
            prs: numbers.iter().copied().map(pr).collect(),
            load: LoadState::Loaded { at: Utc::now() },
            collapsed: false,
        }
    }

    fn selection(repo: &str, number: u32) -> Selection {
        Selection {
            repo: repo.parse().expect("valid repo id"),
            pr: PrNumber(number),
        }
    }

    /// Two repos of two PRs each: header, pr, pr, spacer, header, pr, pr, spacer.
    fn two_repos() -> (Vec<RepoState>, Feed) {
        let repos = vec![repo("a/b", &[1, 2]), repo("c/d", &[3, 4])];
        let feed = flatten(&repos, &FeedFilter::default());
        (repos, feed)
    }

    #[test]
    fn selection_resolves_to_its_row() {
        let (repos, feed) = two_repos();
        assert_eq!(
            selected_row(&feed, &repos, Some(&selection("c/d", 3))),
            Some(5)
        );
    }

    #[test]
    fn absent_selection_resolves_to_no_row() {
        let (repos, feed) = two_repos();
        assert_eq!(selected_row(&feed, &repos, None), None);
    }

    /// A PR that closed between refreshes, and a repo dropped from the config,
    /// must both degrade to "no row" rather than to a stale index.
    #[test]
    fn vanished_selection_resolves_to_no_row() {
        let (repos, feed) = two_repos();
        assert_eq!(
            selected_row(&feed, &repos, Some(&selection("a/b", 99))),
            None
        );
        assert_eq!(
            selected_row(&feed, &repos, Some(&selection("x/y", 1))),
            None
        );
    }

    /// Filtered-out and collapsed pull requests still exist in state but own no
    /// row, so navigation must treat them as unselected.
    #[test]
    fn hidden_selection_resolves_to_no_row() {
        let repos = vec![repo("a/b", &[1, 2])];

        let filtered = flatten(
            &repos,
            &FeedFilter {
                query: "PR 2".into(),
                hide_drafts: false,
                hide_empty_repos: false,
            },
        );
        assert_eq!(
            selected_row(&filtered, &repos, Some(&selection("a/b", 1))),
            None
        );

        let mut collapsed_repos = repos.clone();
        collapsed_repos[0].collapsed = true;
        let collapsed = flatten(&collapsed_repos, &FeedFilter::default());
        assert_eq!(
            selected_row(&collapsed, &collapsed_repos, Some(&selection("a/b", 1))),
            None
        );
    }

    #[test]
    fn next_and_previous_walk_pr_rows_across_repos() {
        let (_, feed) = two_repos();
        // Rows 1, 2, 5, 6 are the PR rows.
        assert_eq!(navigate(&feed, Some(1), Nav::Next), Some(2));
        assert_eq!(navigate(&feed, Some(2), Nav::Next), Some(5));
        assert_eq!(navigate(&feed, Some(5), Nav::Previous), Some(2));
        assert_eq!(navigate(&feed, Some(2), Nav::Previous), Some(1));
    }

    #[test]
    fn first_and_last_target_the_outermost_pr_rows() {
        let (_, feed) = two_repos();
        assert_eq!(navigate(&feed, Some(5), Nav::First), Some(1));
        assert_eq!(navigate(&feed, Some(1), Nav::Last), Some(6));
    }

    /// Navigation must not wrap: pinning at the ends keeps a held key from
    /// jumping the selection from the bottom of the feed back to the top.
    #[test]
    fn navigation_does_not_wrap_at_the_ends() {
        let (_, feed) = two_repos();
        assert_eq!(navigate(&feed, Some(6), Nav::Next), Some(6));
        assert_eq!(navigate(&feed, Some(1), Nav::Previous), Some(1));
    }

    #[test]
    fn no_selection_enters_the_feed_from_the_matching_end() {
        let (_, feed) = two_repos();
        assert_eq!(navigate(&feed, None, Nav::Next), Some(1));
        assert_eq!(navigate(&feed, None, Nav::Previous), Some(6));
        assert_eq!(navigate(&feed, None, Nav::First), Some(1));
        assert_eq!(navigate(&feed, None, Nav::Last), Some(6));
    }

    #[test]
    fn empty_feed_has_nowhere_to_go() {
        let feed = flatten(&[], &FeedFilter::default());
        for nav in [Nav::Next, Nav::Previous, Nav::First, Nav::Last] {
            assert_eq!(navigate(&feed, None, nav), None, "{nav:?}");
        }
    }

    /// A feed of nothing but headers, notices and spacers has no landing spot,
    /// and must not select one of them.
    #[test]
    fn feed_without_pr_rows_has_nowhere_to_go() {
        let repos = vec![repo("a/b", &[]), repo("c/d", &[])];
        // Empty repos are hidden by default; show them so the feed actually has
        // rows, which is the case under test.
        let feed = flatten(
            &repos,
            &FeedFilter {
                hide_empty_repos: false,
                ..Default::default()
            },
        );
        assert!(!feed.is_empty());
        assert!(
            !feed
                .rows()
                .iter()
                .any(|row| matches!(row, FeedRow::PrRow { .. }))
        );
        for nav in [Nav::Next, Nav::Previous, Nav::First, Nav::Last] {
            assert_eq!(navigate(&feed, None, nav), None, "{nav:?}");
        }
    }

    /// Filtering shrinks the navigable set; navigation must follow the visible
    /// rows, never the underlying pull request vector.
    #[test]
    fn navigation_follows_the_filtered_rows() {
        let repos = vec![repo("a/b", &[1, 2, 3])];
        let feed = flatten(
            &repos,
            &FeedFilter {
                query: "PR 2".into(),
                hide_drafts: false,
                hide_empty_repos: false,
            },
        );
        assert_eq!(navigate(&feed, None, Nav::First), Some(1));
        assert_eq!(navigate(&feed, Some(1), Nav::Next), Some(1));
        assert_eq!(
            selected_row(&feed, &repos, Some(&selection("a/b", 2))),
            Some(1)
        );
    }
}
