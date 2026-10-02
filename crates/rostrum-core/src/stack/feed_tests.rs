//! Stacks in the flattened feed: a header, then the members bottom first,
//! contiguous inside their repository's run — and, under a sort, ordered as
//! one unit.
//!
//! The structural tests lay the feed out as listed, so they say nothing about
//! sorting; the sort tests at the bottom use real `FeedSort` keys.

use chrono::Utc;

use crate::{
    feed::{Chrome, Feed, FeedFilter, FeedRow, PrIx, RepoIx, StackPlace, StackSlot, flatten_in},
    model::{PrNumber, PullRequest, RepoId},
    sort::{FeedOrder, FeedSort, ItemSortKey, Sort, SortDirection},
    state::{LoadState, RepoState},
    test_support::pull,
};

use super::{RefName, Stack, StackIx, StackMembers, StackNumber};

/// The feed in the order state lists it: repositories as given, items as
/// fetched.
fn listed(repos: &[RepoState], filter: &FeedFilter) -> Feed {
    flatten_in(repos, filter, FeedOrder::AsListed)
}

fn link(number: u32, head: &str, base: &str) -> PullRequest {
    let mut pr = pull(number);
    pr.head_ref = head.into();
    pr.base_ref = base.into();
    pr
}

fn repo(name: &str, prs: Vec<PullRequest>, stacks: Vec<Stack>) -> RepoState {
    RepoState {
        prs,
        load: LoadState::Loaded { at: Utc::now() },
        stacks,
        ..RepoState::new(name.parse::<RepoId>().expect("valid"))
    }
}

fn github(repo: &str, number: u32, members: &[u32]) -> Stack {
    Stack {
        repo: repo.parse().expect("valid"),
        number: StackNumber::new(number),
        trunk: RefName::new("main").expect("valid"),
        members: StackMembers::new(members.iter().copied().map(PrNumber).collect()).expect("valid"),
    }
}

fn row(pr: usize, slot: Option<(usize, StackPlace)>) -> FeedRow {
    FeedRow::PrRow {
        repo: RepoIx(0),
        pr: PrIx(pr),
        stack: slot.map(|(stack, place)| StackSlot {
            stack: StackIx(stack),
            place,
        }),
    }
}

fn header(stack: usize) -> FeedRow {
    FeedRow::StackHeader {
        repo: RepoIx(0),
        stack: StackIx(stack),
    }
}

#[test]
fn a_detected_chain_renders_as_a_header_and_its_members_bottom_first() {
    // Feed order (updated desc): top, lone, bottom.
    let state = repo(
        "o/r",
        vec![
            link(2, "b", "a"),
            link(9, "z", "main"),
            link(1, "a", "main"),
        ],
        vec![],
    );
    let feed = listed(&[state], &FeedFilter::default());
    assert_eq!(
        feed.rows(),
        &[
            FeedRow::RepoHeader { repo: RepoIx(0) },
            header(0),
            row(2, Some((0, StackPlace::Bottom))),
            row(0, Some((0, StackPlace::Top))),
            row(1, None),
            FeedRow::Spacer { repo: RepoIx(0) },
        ]
    );
    let stack = feed.stack(StackIx(0)).expect("indexed");
    assert_eq!(stack.repo, RepoIx(0));
    assert!(!stack.group.stack.is_on_github());
    assert_eq!(stack.group.stack.trunk.as_str(), "main");
}

#[test]
fn a_github_stack_uses_github_order_and_marks_the_middle() {
    let state = repo(
        "o/r",
        vec![
            link(5, "x", "main"),
            link(6, "y", "main"),
            link(7, "z", "main"),
        ],
        vec![github("o/r", 3, &[6, 7, 5])],
    );
    let feed = listed(&[state], &FeedFilter::default());
    assert_eq!(
        &feed.rows()[1..5],
        &[
            header(0),
            row(1, Some((0, StackPlace::Bottom))),
            row(2, Some((0, StackPlace::Middle))),
            row(0, Some((0, StackPlace::Top))),
        ]
    );
    assert_eq!(
        feed.stack(StackIx(0)).and_then(|s| s.group.stack.number),
        StackNumber::new(3)
    );
}

#[test]
fn stack_rows_stay_inside_the_repository_container() {
    let state = repo("o/r", vec![link(1, "a", "main"), link(2, "b", "a")], vec![]);
    let feed = listed(
        &[state, repo("p/q", vec![pull(4)], vec![])],
        &FeedFilter::default(),
    );
    // Header, stack header, two members, spacer; then the next repo.
    assert_eq!(feed.chrome(0), Chrome::Top);
    assert_eq!(feed.chrome(1), Chrome::Middle);
    assert_eq!(feed.chrome(2), Chrome::Middle);
    assert_eq!(feed.chrome(3), Chrome::Bottom);
    assert_eq!(feed.chrome(4), Chrome::None);
    assert!(matches!(
        feed.row(5),
        Some(FeedRow::RepoHeader { repo: RepoIx(1) })
    ));
}

#[test]
fn stack_indices_are_global_across_repositories() {
    let first = repo("o/r", vec![link(1, "a", "main"), link(2, "b", "a")], vec![]);
    let second = repo("p/q", vec![link(1, "a", "main"), link(2, "b", "a")], vec![]);
    let feed = listed(&[first, second], &FeedFilter::default());
    let headers: Vec<_> = feed
        .rows()
        .iter()
        .filter_map(|row| match row {
            FeedRow::StackHeader { repo, stack } => Some((*repo, *stack)),
            _ => None,
        })
        .collect();
    assert_eq!(
        headers,
        vec![(RepoIx(0), StackIx(0)), (RepoIx(1), StackIx(1))]
    );
    assert_eq!(feed.stacks().len(), 2);
    assert_eq!(feed.stack(StackIx(1)).map(|s| s.repo), Some(RepoIx(1)));
}

#[test]
fn a_filtered_member_leaves_the_rest_of_its_stack_under_the_header() {
    let mut middle = link(2, "b", "a");
    middle.is_draft = true;
    let state = repo(
        "o/r",
        vec![link(1, "a", "main"), middle, link(3, "c", "b")],
        vec![],
    );
    let filter = FeedFilter {
        hide_drafts: true,
        ..Default::default()
    };
    let feed = listed(&[state], &filter);
    assert_eq!(
        &feed.rows()[1..4],
        &[
            header(0),
            row(0, Some((0, StackPlace::Bottom))),
            row(2, Some((0, StackPlace::Top))),
        ]
    );
    // The group still knows all three.
    let group = &feed.stack(StackIx(0)).expect("indexed").group;
    assert_eq!(group.open.len(), 3);
}

#[test]
fn one_visible_member_is_marked_only() {
    let state = repo("o/r", vec![link(1, "a", "main"), link(2, "b", "a")], vec![]);
    let filter = FeedFilter {
        query: "PR 2".into(),
        ..Default::default()
    };
    let feed = listed(&[state], &filter);
    assert_eq!(
        &feed.rows()[1..3],
        &[header(0), row(1, Some((0, StackPlace::Only)))]
    );
}

#[test]
fn a_stack_filtered_out_entirely_has_no_header() {
    let state = repo(
        "o/r",
        vec![
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(3, "z", "main"),
        ],
        vec![],
    );
    let filter = FeedFilter {
        query: "PR 3".into(),
        ..Default::default()
    };
    let feed = listed(&[state], &filter);
    assert!(feed.stacks().is_empty());
    assert_eq!(feed.rows()[1], row(2, None));
}

#[test]
fn a_collapsed_repository_has_no_stack_rows() {
    let mut state = repo("o/r", vec![link(1, "a", "main"), link(2, "b", "a")], vec![]);
    state.collapsed = true;
    let feed = listed(&[state], &FeedFilter::default());
    assert_eq!(feed.len(), 2);
    assert!(feed.stacks().is_empty());
}

#[test]
fn a_github_stack_with_only_merged_members_left_has_no_header() {
    let state = repo("o/r", vec![pull(9)], vec![github("o/r", 1, &[1, 2])]);
    let feed = listed(&[state], &FeedFilter::default());
    assert!(feed.stacks().is_empty());
}

#[test]
fn every_member_is_rendered_exactly_once() {
    let state = repo(
        "o/r",
        vec![
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(3, "c", "main"),
            link(4, "d", "c"),
            link(5, "e", "main"),
        ],
        vec![github("o/r", 2, &[3, 4])],
    );
    let feed = listed(&[state], &FeedFilter::default());
    let mut seen: Vec<usize> = feed
        .rows()
        .iter()
        .filter_map(|row| match row {
            FeedRow::PrRow { pr, .. } => Some(pr.0),
            _ => None,
        })
        .collect();
    seen.sort_unstable();
    assert_eq!(seen, vec![0, 1, 2, 3, 4]);
    assert_eq!(feed.stacks().len(), 2);
}

// --- sorting a stack as one unit ---------------------------------------------

fn at(secs: i64) -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp(secs, 0).expect("valid")
}

/// A chain whose bottom (#1, "zeta") is the oldest and whose top (#2,
/// "alpha") is the newest, beside a lone #3 ("middle") created in between.
fn sortable() -> RepoState {
    let mut bottom = link(1, "a", "main");
    bottom.title = "zeta".into();
    bottom.created_at = at(100);
    let mut top = link(2, "b", "a");
    top.title = "alpha".into();
    top.created_at = at(300);
    let mut lone = link(3, "z", "main");
    lone.title = "middle".into();
    lone.created_at = at(200);
    repo("o/r", vec![lone, top, bottom], vec![])
}

fn sorted(key: ItemSortKey, direction: SortDirection) -> Vec<FeedRow> {
    let filter = FeedFilter {
        sort: FeedSort {
            items: Sort::with_direction(key, direction),
            ..FeedSort::default()
        },
        ..FeedFilter::default()
    };
    let feed = flatten_in(&[sortable()], &filter, FeedOrder::Sorted(filter.sort));
    feed.rows()[1..feed.len() - 1].to_vec()
}

/// Rows of `sortable()`: index 0 is the lone #3, 1 the top #2, 2 the bottom #1.
fn stack_then_lone() -> Vec<FeedRow> {
    vec![
        header(0),
        row(2, Some((0, StackPlace::Bottom))),
        row(1, Some((0, StackPlace::Top))),
        row(0, None),
    ]
}

fn lone_then_stack() -> Vec<FeedRow> {
    vec![
        row(0, None),
        header(0),
        row(2, Some((0, StackPlace::Bottom))),
        row(1, Some((0, StackPlace::Top))),
    ]
}

#[test]
fn newest_first_files_a_stack_under_its_newest_member() {
    // The top (300) is newer than the lone pull request (200).
    assert_eq!(
        sorted(ItemSortKey::Created, SortDirection::Descending),
        stack_then_lone()
    );
}

#[test]
fn oldest_first_files_a_stack_under_its_oldest_member() {
    // The bottom (100) is older than the lone pull request (200).
    assert_eq!(
        sorted(ItemSortKey::Created, SortDirection::Ascending),
        stack_then_lone()
    );
}

#[test]
fn alphabetical_files_a_stack_under_its_bottom_member() {
    // "middle" < "zeta" (the bottom), even though the top's "alpha" would
    // sort first on its own.
    assert_eq!(
        sorted(ItemSortKey::Title, SortDirection::Ascending),
        lone_then_stack()
    );
    assert_eq!(
        sorted(ItemSortKey::Title, SortDirection::Descending),
        stack_then_lone()
    );
}

#[test]
fn members_stay_bottom_first_whatever_the_sort() {
    for key in [
        ItemSortKey::Created,
        ItemSortKey::Title,
        ItemSortKey::Updated,
    ] {
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            let rows = sorted(key, direction);
            let members: Vec<usize> = rows
                .iter()
                .filter_map(|row| match row {
                    FeedRow::PrRow {
                        pr, stack: Some(_), ..
                    } => Some(pr.0),
                    _ => None,
                })
                .collect();
            assert_eq!(members, vec![2, 1], "{key:?} {direction:?}");
        }
    }
}

// --- tabs and single-repository views ----------------------------------------

/// Stacks are pull requests only: the Issues tab of a repository with a
/// stack has no stack header and lists its issues one row each.
#[test]
fn the_issues_tab_has_no_stack_rows() {
    let mut state = sortable();
    state.issues = vec![crate::test_support::issue(7), crate::test_support::issue(8)];
    state.issues_load = LoadState::Loaded { at: Utc::now() };
    let feed = crate::feed::flatten_tab(
        &[state],
        &FeedFilter::default(),
        crate::tabs::FeedTab::Issues,
    );
    assert!(feed.stacks().is_empty());
    assert!(
        feed.rows()
            .iter()
            .all(|row| !matches!(row, FeedRow::StackHeader { .. } | FeedRow::PrRow { .. }))
    );
    assert_eq!(
        feed.rows()
            .iter()
            .filter(|row| matches!(row, FeedRow::IssueRow { .. }))
            .count(),
        2
    );
}

/// A single repository's rows match its run in the feed, sorted the same way,
/// with stacks grouped — and are produced even when the repository is
/// collapsed in the feed or the feed's filter would hide them.
#[test]
fn repo_pull_rows_lay_one_repository_out_like_the_feed() {
    let mut state = sortable();
    state.collapsed = true;
    let sort = Sort::with_direction(ItemSortKey::Created, SortDirection::Descending);
    let (rows, stacks) = crate::feed::repo_pull_rows(&state, sort);
    assert_eq!(rows, stack_then_lone());
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].group.open, vec![PrIx(2), PrIx(1)]);

    // By title the stack files under its bottom member, "zeta", after the
    // lone "middle".
    let (rows, _) = crate::feed::repo_pull_rows(
        &state,
        Sort::with_direction(ItemSortKey::Title, SortDirection::Ascending),
    );
    assert_eq!(rows, lone_then_stack());
}
