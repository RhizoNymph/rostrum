//! The item sort over issues: the same keys, directions and tie-breaks as
//! pull requests, with "pushed" falling back to "updated".

use super::*;
use crate::{
    feed::{IssueIx, flatten_tab, flatten_tab_in},
    issue::Issue,
    tabs::FeedTab,
    test_support::issue,
};

/// An issue whose every sortable field is pinned.
fn fixed(number: u32) -> Issue {
    Issue {
        created_at: at(0),
        updated_at: at(0),
        ..issue(number)
    }
}

fn issue_created(number: u32, secs: i64) -> Issue {
    Issue {
        created_at: at(secs),
        ..fixed(number)
    }
}

fn issue_updated(number: u32, secs: i64) -> Issue {
    Issue {
        updated_at: at(secs),
        ..fixed(number)
    }
}

fn issue_titled(number: u32, title: &str) -> Issue {
    Issue {
        title: title.into(),
        ..fixed(number)
    }
}

fn issue_authored(number: u32, login: Option<&str>) -> Issue {
    Issue {
        author: login.map(|login| User {
            login: login.into(),
            avatar_url: None,
        }),
        ..fixed(number)
    }
}

fn issue_order(issues: &[Issue], sort: Sort<ItemSortKey>) -> Vec<u32> {
    let mut indices: Vec<IssueIx> = (0..issues.len()).map(IssueIx).collect();
    order_issues(issues, &mut indices, sort);
    indices
        .into_iter()
        .map(|IssueIx(ix)| issues[ix].number.0)
        .collect()
}

fn with_issues(mut state: RepoState, issues: Vec<Issue>) -> RepoState {
    state.issues = issues;
    state.issues_load = LoadState::Loaded { at: at(0) };
    state
}

fn issue_numbers(feed: &crate::Feed, repos: &[RepoState]) -> Vec<(String, u32)> {
    feed.rows()
        .iter()
        .filter_map(|row| match *row {
            FeedRow::IssueRow { repo, issue } => {
                let state = &repos[repo.0];
                Some((state.id.to_string(), state.issues[issue.0].number.0))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn issues_by_created() {
    let issues = [
        issue_created(1, 20),
        issue_created(2, 10),
        issue_created(3, 30),
    ];
    assert_eq!(issue_order(&issues, desc(ItemSortKey::Created)), [3, 1, 2]);
    assert_eq!(issue_order(&issues, asc(ItemSortKey::Created)), [2, 1, 3]);
}

#[test]
fn issues_by_updated() {
    let issues = [
        issue_updated(1, 5),
        issue_updated(2, 50),
        issue_updated(3, 20),
    ];
    assert_eq!(issue_order(&issues, desc(ItemSortKey::Updated)), [2, 3, 1]);
    assert_eq!(issue_order(&issues, asc(ItemSortKey::Updated)), [1, 3, 2]);
}

/// Issues have no branch to push to; "pushed" orders them exactly as
/// "updated" does rather than leaving every issue unknown.
#[test]
fn issues_by_pushed_fall_back_to_updated() {
    let issues = [
        issue_updated(1, 5),
        issue_updated(2, 50),
        issue_updated(3, 20),
    ];
    for direction in [SortDirection::Descending, SortDirection::Ascending] {
        assert_eq!(
            issue_order(
                &issues,
                Sort::with_direction(ItemSortKey::Pushed, direction)
            ),
            issue_order(
                &issues,
                Sort::with_direction(ItemSortKey::Updated, direction)
            ),
        );
    }
    for issue in &issues {
        assert_eq!(
            issue_sort_value(issue, ItemSortKey::Pushed),
            Some(SortValue::Time(issue.updated_at))
        );
    }
}

#[test]
fn issues_by_title_and_author_ignore_case() {
    let titled = [
        issue_titled(1, "beta"),
        issue_titled(2, "Alpha"),
        issue_titled(3, "gamma"),
    ];
    assert_eq!(issue_order(&titled, asc(ItemSortKey::Title)), [2, 1, 3]);

    let authored = [
        issue_authored(1, Some("zoe")),
        issue_authored(2, Some("Adam")),
        issue_authored(3, Some("mia")),
    ];
    assert_eq!(issue_order(&authored, asc(ItemSortKey::Author)), [2, 3, 1]);
}

/// A deleted account has no login: the issue sinks in both directions, as
/// a pull request does.
#[test]
fn issues_without_an_author_go_last_in_both_directions() {
    let issues = [
        issue_authored(1, None),
        issue_authored(2, Some("bo")),
        issue_authored(3, Some("al")),
    ];
    assert_eq!(issue_order(&issues, asc(ItemSortKey::Author)), [3, 2, 1]);
    assert_eq!(issue_order(&issues, desc(ItemSortKey::Author)), [2, 3, 1]);
}

#[test]
fn issue_ties_fall_back_to_the_number_whatever_the_direction() {
    let issues = [
        issue_titled(9, "same"),
        issue_titled(2, "same"),
        issue_titled(5, "same"),
    ];
    for key in ItemSortKey::ALL.iter().copied() {
        assert_eq!(issue_order(&issues, asc(key)), [2, 5, 9], "{key:?}");
        assert_eq!(issue_order(&issues, desc(key)), [2, 5, 9], "{key:?}");
    }
}

/// A repository's "updated" is its newest open item of either kind, so a
/// repository busy only with issues is not ranked as idle.
#[test]
fn repos_by_updated_count_issues_too() {
    let mut prs_only = with_meta("o/prs", |m| m.updated_at = at(1));
    prs_only.prs = vec![updated(1, 20)];
    let issues_only = with_issues(
        with_meta("o/issues", |m| m.updated_at = at(1)),
        vec![issue_updated(1, 40)],
    );
    let repos = [prs_only, issues_only];
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Updated)),
        ["o/issues", "o/prs"]
    );
}

#[test]
fn the_issues_tab_follows_both_sorts() {
    let quiet = with_issues(
        with_meta("o/quiet", |m| m.pushed_at = Some(at(1))),
        vec![issue_created(1, 1), issue_created(2, 2)],
    );
    let busy = with_issues(
        with_meta("o/busy", |m| m.pushed_at = Some(at(9))),
        vec![
            issue_created(7, 5),
            issue_created(8, 3),
            issue_created(9, 4),
        ],
    );
    let repos = [quiet, busy];

    let feed = flatten_tab(&repos, &FeedFilter::default(), FeedTab::Issues);
    assert_eq!(
        issue_numbers(&feed, &repos),
        [
            ("o/busy".to_string(), 7),
            ("o/busy".to_string(), 9),
            ("o/busy".to_string(), 8),
            ("o/quiet".to_string(), 2),
            ("o/quiet".to_string(), 1),
        ]
    );

    let filter = FeedFilter {
        sort: FeedSort {
            repos: asc(RepoSortKey::Name),
            items: asc(ItemSortKey::Created),
        },
        ..FeedFilter::default()
    };
    let feed = flatten_tab(&repos, &filter, FeedTab::Issues);
    assert_eq!(
        issue_numbers(&feed, &repos),
        [
            ("o/busy".to_string(), 8),
            ("o/busy".to_string(), 9),
            ("o/busy".to_string(), 7),
            ("o/quiet".to_string(), 1),
            ("o/quiet".to_string(), 2),
        ]
    );
}

#[test]
fn listed_order_keeps_issues_as_fetched() {
    let repos = [
        with_issues(
            repo("o/b"),
            vec![
                issue_created(3, 1),
                issue_created(1, 9),
                issue_created(2, 5),
            ],
        ),
        with_issues(repo("o/a"), vec![issue_created(4, 0)]),
    ];
    let feed = flatten_tab_in(
        &repos,
        &FeedFilter::default(),
        FeedTab::Issues,
        FeedOrder::AsListed,
    );
    assert_eq!(
        issue_numbers(&feed, &repos),
        [
            ("o/b".to_string(), 3),
            ("o/b".to_string(), 1),
            ("o/b".to_string(), 2),
            ("o/a".to_string(), 4),
        ]
    );
}
