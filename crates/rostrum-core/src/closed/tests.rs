use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};

use super::*;
use crate::{
    feed::{Chrome, FeedRow, PrIx, flatten_tab, flatten_tab_in},
    model::Label,
    sort::{FeedOrder, FeedSort},
    tabs::{FeedTab, tab_counts},
    test_support::{fixed_time, pull},
};

/// `minutes` after the fixed instant every fixture shares.
fn t(minutes: i64) -> DateTime<Utc> {
    fixed_time() + Duration::minutes(minutes)
}

fn user(login: &str) -> User {
    User {
        login: login.into(),
        avatar_url: None,
    }
}

fn merged(number: u32, at: i64) -> ClosedPull {
    ClosedPull {
        pull: pull(number),
        ending: PullEnding::Merged { at: t(at) },
    }
}

fn closed(number: u32, at: i64) -> ClosedPull {
    ClosedPull {
        pull: pull(number),
        ending: PullEnding::Closed { at: t(at) },
    }
}

fn loaded() -> LoadState {
    LoadState::Loaded { at: Utc::now() }
}

fn failed() -> LoadState {
    LoadState::Failed {
        message: "boom".into(),
        at: Utc::now(),
    }
}

/// A repository whose closed list has loaded.
fn repo(name: &str, closed: Vec<ClosedPull>) -> RepoState {
    let mut state = RepoState::new(name.parse().expect("valid repo id"));
    state.closed = closed;
    state.closed_load = loaded();
    state
}

fn row(repo: usize, closed: usize) -> FeedRow {
    FeedRow::ClosedRow {
        repo: RepoIx(repo),
        closed: ClosedIx(closed),
    }
}

fn numbers(repos: &[RepoState], filter: &FeedFilter) -> Vec<(String, u32)> {
    closed_order(repos, filter)
        .into_iter()
        .map(|(repo, closed)| {
            let state = &repos[repo.0];
            (state.id.to_string(), state.closed[closed.0].number().0)
        })
        .collect()
}

// --- the ending ------------------------------------------------------------

#[test]
fn a_merged_pull_request_ends_at_its_merge_time() {
    let ending = PullEnding::from_parts(PullState::Merged, Some(t(5)), Some(t(5)));
    assert_eq!(ending, Ok(PullEnding::Merged { at: t(5) }));
}

/// GitHub stamps `closedAt` on a merge too, sometimes a moment apart; the
/// merge time is the one that counts.
#[test]
fn a_merge_ignores_the_close_time_beside_it() {
    let ending = PullEnding::from_parts(PullState::Merged, Some(t(5)), Some(t(9)));
    assert_eq!(ending.map(PullEnding::at), Ok(t(5)));
}

#[test]
fn a_closed_pull_request_ends_at_its_close_time() {
    let ending = PullEnding::from_parts(PullState::Closed, None, Some(t(3)));
    assert_eq!(ending, Ok(PullEnding::Closed { at: t(3) }));
}

/// A stray `mergedAt` on a closed pull request cannot turn it into a merge:
/// the state decides the kind.
#[test]
fn the_state_decides_the_kind_not_which_times_are_present() {
    let ending = PullEnding::from_parts(PullState::Closed, Some(t(1)), Some(t(3)));
    assert_eq!(ending, Ok(PullEnding::Closed { at: t(3) }));
}

#[test]
fn inconsistent_fields_are_rejected_with_the_reason() {
    assert_eq!(
        PullEnding::from_parts(PullState::Open, None, None),
        Err(EndingError::StillOpen)
    );
    assert_eq!(
        PullEnding::from_parts(PullState::Merged, None, Some(t(1))),
        Err(EndingError::MergedWithoutTime)
    );
    assert_eq!(
        PullEnding::from_parts(PullState::Closed, Some(t(1)), None),
        Err(EndingError::ClosedWithoutTime)
    );
}

#[test]
fn an_ending_reports_its_kind_state_and_word() {
    let merge = PullEnding::Merged { at: t(0) };
    let close = PullEnding::Closed { at: t(0) };
    assert!(merge.is_merged());
    assert!(!close.is_merged());
    assert_eq!(merge.state(), PullState::Merged);
    assert_eq!(close.state(), PullState::Closed);
    assert_eq!(merge.verb(), "merged");
    assert_eq!(close.verb(), "closed");
}

/// The cache stores closed pull requests as JSON; both kinds must survive
/// the trip unchanged.
#[test]
fn closed_pull_requests_round_trip_through_json() {
    for original in [merged(1, 4), closed(2, 7)] {
        let text = serde_json::to_string(&original).expect("encodes");
        let back: ClosedPull = serde_json::from_str(&text).expect("decodes");
        assert_eq!(back, original);
    }
    let text = serde_json::to_string(&PullEnding::Merged { at: t(0) }).expect("encodes");
    assert!(text.starts_with(r#"{"merged":"#), "{text}");
}

// --- ordering --------------------------------------------------------------

/// The tab's whole point: one list, merges and closes interleaved, newest
/// ending first, across repositories.
#[test]
fn merges_and_closes_interleave_newest_ending_first_across_repositories() {
    let repos = [
        repo("a/one", vec![merged(1, 10), closed(2, 30)]),
        repo("b/two", vec![closed(3, 20), merged(4, 40)]),
    ];
    assert_eq!(
        numbers(&repos, &FeedFilter::default()),
        vec![
            ("b/two".into(), 4),
            ("a/one".into(), 2),
            ("b/two".into(), 3),
            ("a/one".into(), 1),
        ]
    );
}

/// The fetch arrives in update order; a pull request touched long after it
/// merged must still sort by its merge.
#[test]
fn the_order_ignores_when_a_pull_request_was_last_updated() {
    let mut late_comment = merged(1, 10);
    late_comment.pull.updated_at = t(1_000);
    let mut early = merged(2, 20);
    early.pull.updated_at = t(20);
    let repos = [repo("a/b", vec![late_comment, early])];
    assert_eq!(
        numbers(&repos, &FeedFilter::default()),
        vec![("a/b".into(), 2), ("a/b".into(), 1)]
    );
}

/// A merge queue lands several pull requests in the same second; the order
/// must not depend on which repository answered first.
#[test]
fn ties_break_on_the_repository_then_the_higher_number() {
    let repos = [
        repo("z/last", vec![merged(1, 5)]),
        repo("a/first", vec![merged(7, 5), merged(9, 5)]),
    ];
    assert_eq!(
        numbers(&repos, &FeedFilter::default()),
        vec![
            ("a/first".into(), 9),
            ("a/first".into(), 7),
            ("z/last".into(), 1),
        ]
    );
}

/// The feed's sort dropdown is for the open tabs; the Closed tab's order is
/// fixed whatever it says.
#[test]
fn the_feed_sort_does_not_reorder_the_closed_tab() {
    let repos = [
        repo("a/one", vec![merged(1, 10)]),
        repo("b/two", vec![merged(2, 20)]),
    ];
    let mut reversed = FeedFilter::default();
    reversed.sort.items.reverse();
    reversed.sort.repos.reverse();
    assert_ne!(reversed.sort, FeedSort::default());
    let expected = flatten_tab(&repos, &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(flatten_tab(&repos, &reversed, FeedTab::Closed), expected);
    assert_eq!(
        flatten_tab_in(&repos, &reversed, FeedTab::Closed, FeedOrder::AsListed),
        expected
    );
}

// --- filtering -------------------------------------------------------------

fn authored(number: u32, at: i64, login: &str) -> ClosedPull {
    let mut closed = merged(number, at);
    closed.pull.author = Some(user(login));
    closed
}

#[test]
fn the_author_filter_applies_to_closed_pull_requests() {
    let repos = [repo(
        "a/b",
        vec![authored(1, 1, "alice"), authored(2, 2, "bob")],
    )];
    let filter = FeedFilter {
        authors: BTreeSet::from([LoginKey::new("Alice")]),
        ..Default::default()
    };
    assert_eq!(numbers(&repos, &filter), vec![("a/b".into(), 1)]);
    assert_eq!(closed_count(&repos, &filter), 1);
}

#[test]
fn include_involved_widens_to_assignees_and_reviewers() {
    let mut assigned = authored(1, 1, "alice");
    assigned.pull.assignees = vec![user("me")];
    let mut reviewed = authored(2, 2, "bob");
    reviewed.pull.review_requests = vec![user("me")];
    let repos = [repo(
        "a/b",
        vec![assigned, reviewed, authored(3, 3, "carol")],
    )];

    let narrow = FeedFilter {
        authors: BTreeSet::from([LoginKey::new("me")]),
        ..Default::default()
    };
    assert!(numbers(&repos, &narrow).is_empty());
    let wide = FeedFilter {
        include_involved: true,
        ..narrow
    };
    assert_eq!(
        numbers(&repos, &wide),
        vec![("a/b".into(), 2), ("a/b".into(), 1)]
    );
}

#[test]
fn the_search_matches_title_number_author_and_label() {
    let mut titled = merged(11, 1);
    titled.pull.title = "Fix the renderer".into();
    let mut labelled = merged(12, 2);
    labelled.pull.labels = vec![Label {
        name: "needle".into(),
        color: "000000".into(),
    }];
    let repos = [repo("a/b", vec![titled, labelled, merged(13, 3)])];
    let search = |query: &str| {
        numbers(
            &repos,
            &FeedFilter {
                query: query.into(),
                ..Default::default()
            },
        )
    };
    assert_eq!(search("renderer"), vec![("a/b".into(), 11)]);
    assert_eq!(search("NEEDLE"), vec![("a/b".into(), 12)]);
    assert_eq!(search("13"), vec![("a/b".into(), 13)]);
}

/// With no repository headers on this tab, the repository is searchable.
#[test]
fn the_search_also_matches_the_repository() {
    let repos = [
        repo("zed-industries/zed", vec![merged(1, 1)]),
        repo("rust-lang/rust", vec![merged(2, 2)]),
    ];
    let filter = FeedFilter {
        query: "Zed-Ind".into(),
        ..Default::default()
    };
    assert_eq!(
        numbers(&repos, &filter),
        vec![("zed-industries/zed".into(), 1)]
    );
}

/// A closed draft is history, not work in progress.
#[test]
fn hiding_drafts_does_not_hide_closed_pull_requests() {
    let mut draft = closed(1, 1);
    draft.pull.is_draft = true;
    let repos = [repo("a/b", vec![draft])];
    let filter = FeedFilter {
        hide_drafts: true,
        ..Default::default()
    };
    assert_eq!(numbers(&repos, &filter), vec![("a/b".into(), 1)]);
}

// --- the row stream --------------------------------------------------------

#[test]
fn the_closed_tab_is_one_list_then_a_spacer() {
    let repos = [
        repo("a/one", vec![merged(1, 10)]),
        repo("b/two", vec![closed(2, 20), merged(3, 5)]),
    ];
    let feed = flatten_tab(&repos, &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(feed.tab(), FeedTab::Closed);
    assert_eq!(
        feed.rows(),
        &[
            row(1, 0),
            row(0, 0),
            row(1, 1),
            FeedRow::Spacer { repo: RepoIx(1) },
        ]
    );
    assert_eq!(feed.hidden_repos(), 0);
    assert!(feed.stacks().is_empty());
}

/// Closed rows from different repositories draw one container, not a card
/// each.
#[test]
fn closed_rows_share_one_container_across_repositories() {
    let repos = [
        repo("a/one", vec![merged(1, 10)]),
        repo("b/two", vec![closed(2, 20), merged(3, 5)]),
    ];
    let feed = flatten_tab(&repos, &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(feed.chrome(0), Chrome::Top);
    assert_eq!(feed.chrome(1), Chrome::Middle);
    assert_eq!(feed.chrome(2), Chrome::Bottom);
    assert_eq!(feed.chrome(3), Chrome::None);
}

#[test]
fn a_lone_closed_row_is_a_solo_container() {
    let feed = flatten_tab(
        &[repo("a/b", vec![merged(1, 1)])],
        &FeedFilter::default(),
        FeedTab::Closed,
    );
    assert_eq!(feed.chrome(0), Chrome::Solo);
}

#[test]
fn closed_rows_are_items_for_keyboard_navigation() {
    assert!(row(0, 0).is_item());
    assert_eq!(row(3, 1).repo(), RepoIx(3));
}

/// Rows address the unfiltered vector, so a row resolves to its pull request
/// whatever the filter hid.
#[test]
fn closed_indices_address_the_unfiltered_vector() {
    let repos = [repo(
        "a/b",
        vec![
            authored(1, 3, "bob"),
            authored(2, 2, "bob"),
            authored(3, 1, "alice"),
        ],
    )];
    let feed = flatten_tab(
        &repos,
        &FeedFilter {
            authors: BTreeSet::from([LoginKey::new("alice")]),
            ..Default::default()
        },
        FeedTab::Closed,
    );
    assert_eq!(feed.row(0), Some(row(0, 2)));
}

#[test]
fn a_repository_still_loading_with_nothing_gets_a_notice_after_the_list() {
    let mut waiting = repo("c/d", Vec::new());
    waiting.closed_load = LoadState::Loading;
    let mut never = repo("e/f", Vec::new());
    never.closed_load = LoadState::Idle;
    let repos = [repo("a/b", vec![merged(1, 1)]), waiting, never];
    let feed = flatten_tab(&repos, &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(
        feed.rows(),
        &[
            row(0, 0),
            FeedRow::Spacer { repo: RepoIx(0) },
            FeedRow::RepoLoading { repo: RepoIx(1) },
            FeedRow::Spacer { repo: RepoIx(1) },
            FeedRow::RepoLoading { repo: RepoIx(2) },
            FeedRow::Spacer { repo: RepoIx(2) },
        ]
    );
    // Notices are containers of their own, not part of the list's.
    assert_eq!(feed.chrome(0), Chrome::Solo);
    assert_eq!(feed.chrome(2), Chrome::Solo);
}

/// A refresh in flight over a list already held is not worth a notice: the
/// list is showing.
#[test]
fn a_background_refresh_over_held_data_shows_no_notice() {
    let mut state = repo("a/b", vec![merged(1, 1)]);
    state.closed_load = LoadState::Loading;
    let feed = flatten_tab(&[state], &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(
        feed.rows(),
        &[row(0, 0), FeedRow::Spacer { repo: RepoIx(0) }]
    );
}

/// There is no header to carry an error chip on this tab, so a failure is
/// reported even when an older list is still showing.
#[test]
fn a_failed_repository_is_reported_even_with_a_stale_list() {
    let mut stale = repo("a/b", vec![merged(1, 1)]);
    stale.closed_load = failed();
    let mut empty = repo("c/d", Vec::new());
    empty.closed_load = failed();
    let feed = flatten_tab(&[stale, empty], &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(
        feed.rows(),
        &[
            row(0, 0),
            FeedRow::Spacer { repo: RepoIx(0) },
            FeedRow::RepoError { repo: RepoIx(0) },
            FeedRow::Spacer { repo: RepoIx(0) },
            FeedRow::RepoError { repo: RepoIx(1) },
            FeedRow::Spacer { repo: RepoIx(1) },
        ]
    );
    // The error's container is not merged into the list's above it.
    assert_eq!(feed.chrome(2), Chrome::Solo);
}

/// A repository with nothing closed adds nothing: no header, no "empty"
/// notice, nothing hidden.
#[test]
fn a_loaded_repository_with_nothing_closed_adds_no_rows() {
    let feed = flatten_tab(
        &[repo("a/b", Vec::new())],
        &FeedFilter {
            hide_empty_repos: false,
            ..Default::default()
        },
        FeedTab::Closed,
    );
    assert!(feed.is_empty());
    assert_eq!(feed.hidden_repos(), 0);
}

/// Collapse is a per-repository container affordance; the Closed tab has no
/// containers per repository, so a collapsed repository still lists.
#[test]
fn collapse_does_not_hide_closed_rows() {
    let mut state = repo("a/b", vec![merged(1, 1)]);
    state.collapsed = true;
    let feed = flatten_tab(&[state], &FeedFilter::default(), FeedTab::Closed);
    assert_eq!(feed.row(0), Some(row(0, 0)));
}

/// The open tabs never show closed pull requests, and the Closed tab never
/// shows open ones.
#[test]
fn each_tab_holds_only_its_own_kind() {
    let mut state = repo("a/b", vec![merged(9, 1)]);
    state.prs = vec![pull(1)];
    state.load = loaded();
    let repos = [state];
    let filter = FeedFilter::default();

    let open = flatten_tab(&repos, &filter, FeedTab::PullRequests);
    assert!(
        !open
            .rows()
            .iter()
            .any(|row| matches!(row, FeedRow::ClosedRow { .. }))
    );
    assert_eq!(
        open.row(1),
        Some(FeedRow::PrRow {
            repo: RepoIx(0),
            pr: PrIx(0),
            stack: None
        })
    );

    let closed_feed = flatten_tab(&repos, &filter, FeedTab::Closed);
    assert!(
        closed_feed
            .rows()
            .iter()
            .all(|row| matches!(row, FeedRow::ClosedRow { .. } | FeedRow::Spacer { .. }))
    );
    assert_ne!(open, closed_feed);
}

// --- counts ----------------------------------------------------------------

#[test]
fn tab_counts_include_the_closed_list_after_filters() {
    let mut first = repo("a/b", vec![authored(1, 1, "alice"), merged(2, 2)]);
    first.prs = vec![pull(5)];
    let second = repo("c/d", vec![closed(3, 3)]);
    let repos = [first, second];

    let counts = tab_counts(&repos, &FeedFilter::default());
    assert_eq!(counts.get(FeedTab::Closed), 3);
    assert_eq!(counts.get(FeedTab::PullRequests), 1);

    let alice = FeedFilter {
        authors: BTreeSet::from([LoginKey::new("alice")]),
        ..Default::default()
    };
    assert_eq!(tab_counts(&repos, &alice).closed, 1);
}

// --- fetching --------------------------------------------------------------

#[test]
fn a_list_never_fetched_or_failed_is_due() {
    let now = Utc::now();
    assert!(closed_needs_fetch(
        &LoadState::Idle,
        now,
        Duration::minutes(5)
    ));
    assert!(closed_needs_fetch(&failed(), now, Duration::minutes(5)));
}

#[test]
fn a_fetch_in_flight_is_not_due_again() {
    assert!(!closed_needs_fetch(
        &LoadState::Loading,
        Utc::now(),
        Duration::minutes(5)
    ));
}

#[test]
fn a_loaded_list_is_due_once_it_is_older_than_the_window() {
    let now = Utc::now();
    let max_age = Duration::minutes(5);
    let fresh = LoadState::Loaded {
        at: now - Duration::minutes(4),
    };
    let stale = LoadState::Loaded {
        at: now - Duration::minutes(5),
    };
    assert!(!closed_needs_fetch(&fresh, now, max_age));
    assert!(closed_needs_fetch(&stale, now, max_age));
}

// --- roster and lookup -----------------------------------------------------

#[test]
fn the_closed_roster_orders_people_by_their_latest_ending() {
    let repos = [repo(
        "a/b",
        vec![
            authored(1, 5, "alice"),
            authored(2, 50, "bob"),
            authored(3, 10, "alice"),
        ],
    )];
    let roster = closed_roster(&repos, None, &BTreeSet::new());
    let logins: Vec<(&str, usize)> = roster
        .iter()
        .map(|entry| (entry.user.login.as_str(), entry.open_prs))
        .collect();
    assert_eq!(logins, vec![("bob", 1), ("alice", 2)]);
    assert_eq!(roster[0].latest, Some(t(50)));
}

#[test]
fn find_closed_looks_up_by_number() {
    let state = repo("a/b", vec![merged(4, 1), closed(8, 2)]);
    assert_eq!(
        find_closed(&state, PrNumber(8)).map(|closed| closed.ending),
        Some(PullEnding::Closed { at: t(2) })
    );
    assert!(find_closed(&state, PrNumber(5)).is_none());
}
