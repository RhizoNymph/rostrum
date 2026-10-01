//! Stacks in the flattened feed: a header, then the members bottom first,
//! contiguous inside their repository's run.

use chrono::Utc;

use crate::{
    feed::{Chrome, FeedFilter, FeedRow, PrIx, RepoIx, StackPlace, StackSlot, flatten},
    model::{PrNumber, PullRequest, RepoId},
    state::{LoadState, RepoState},
    test_support::pull,
};

use super::{RefName, Stack, StackIx, StackMembers, StackNumber};

fn link(number: u32, head: &str, base: &str) -> PullRequest {
    let mut pr = pull(number);
    pr.head_ref = head.into();
    pr.base_ref = base.into();
    pr
}

fn repo(name: &str, prs: Vec<PullRequest>, stacks: Vec<Stack>) -> RepoState {
    RepoState {
        id: name.parse::<RepoId>().expect("valid"),
        prs,
        load: LoadState::Loaded { at: Utc::now() },
        collapsed: false,
        stacks,
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
    let feed = flatten(&[state], &FeedFilter::default());
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
    let feed = flatten(&[state], &FeedFilter::default());
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
    let feed = flatten(
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
    let feed = flatten(&[first, second], &FeedFilter::default());
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
    let feed = flatten(&[state], &filter);
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
    let feed = flatten(&[state], &filter);
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
    let feed = flatten(&[state], &filter);
    assert!(feed.stacks().is_empty());
    assert_eq!(feed.rows()[1], row(2, None));
}

#[test]
fn a_collapsed_repository_has_no_stack_rows() {
    let mut state = repo("o/r", vec![link(1, "a", "main"), link(2, "b", "a")], vec![]);
    state.collapsed = true;
    let feed = flatten(&[state], &FeedFilter::default());
    assert_eq!(feed.len(), 2);
    assert!(feed.stacks().is_empty());
}

#[test]
fn a_github_stack_with_only_merged_members_left_has_no_header() {
    let state = repo("o/r", vec![pull(9)], vec![github("o/r", 1, &[1, 2])]);
    let feed = flatten(&[state], &FeedFilter::default());
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
    let feed = flatten(&[state], &FeedFilter::default());
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
