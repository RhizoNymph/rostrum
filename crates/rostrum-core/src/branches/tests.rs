//! Tree building: placement, nesting, loops, unknown bases, trunk order and
//! missing counts.

use std::collections::BTreeSet;

use super::*;
use crate::{
    model::{Divergence, PrNumber, PullRequest},
    test_support::pull,
};

fn name(raw: &str) -> TrunkName {
    TrunkName::parse(raw).expect("valid test name")
}

fn trunks(default: &str, configured: &[&str]) -> Trunks {
    let existing: BTreeSet<TrunkName> = configured.iter().map(|raw| name(raw)).collect();
    Trunks::resolve(
        name(default),
        &TrunkChoice::Configured(configured.iter().map(|raw| name(raw)).collect()),
        &existing,
    )
}

fn pr(number: u32, base: &str, head: &str) -> PullRequest {
    let mut pr = pull(number);
    pr.base_ref = base.into();
    pr.head_ref = head.into();
    pr
}

fn none() -> BranchCounts {
    BranchCounts::default()
}

/// The tree as `(depth, label)` lines, which reads like the screen does.
fn outline(tree: &BranchTree) -> Vec<String> {
    tree.rows()
        .into_iter()
        .map(|row| match row {
            BranchRow::Trunk { name, pulls, .. } => format!("{name} ({pulls})"),
            BranchRow::OtherBases => "Other bases".to_string(),
            BranchRow::Base { name, pulls } => format!("? {name} ({pulls})"),
            BranchRow::Pull {
                depth,
                number,
                note,
                ..
            } => {
                let note = match note {
                    Some(PullNote::BreaksCycle) => " [cycle]",
                    Some(PullNote::AmbiguousBase) => " [ambiguous]",
                    None => "",
                };
                format!("{}#{}{note}", "  ".repeat(depth), number.0)
            }
        })
        .collect()
}

// --- placement ---------------------------------------------------------------

#[test]
fn an_empty_repository_shows_its_trunks_alone() {
    let tree = build_tree(&trunks("main", &["staging"]), &[], &none());
    assert_eq!(outline(&tree), ["main (0)", "staging (0)"]);
    assert_eq!(tree.pull_count(), 0);
}

#[test]
fn pull_requests_hang_under_the_trunk_they_are_based_on() {
    let tree = build_tree(
        &trunks("main", &["staging"]),
        &[
            pr(3, "staging", "c"),
            pr(1, "main", "a"),
            pr(2, "main", "b"),
        ],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        ["main (2)", "  #1", "  #2", "staging (1)", "  #3"]
    );
}

#[test]
fn siblings_are_ordered_by_number_whatever_the_input_order() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[pr(9, "main", "x"), pr(2, "main", "y"), pr(5, "main", "z")],
        &none(),
    );
    assert_eq!(outline(&tree), ["main (3)", "  #2", "  #5", "  #9"]);
}

#[test]
fn trunks_keep_their_resolved_order_with_the_default_first() {
    let tree = build_tree(
        &trunks("main", &["staging", "develop", "release"]),
        &[],
        &none(),
    );
    let names: Vec<&str> = tree.trunks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["main", "staging", "develop", "release"]);
}

// --- stacks -------------------------------------------------------------------

#[test]
fn a_pull_request_based_on_another_ones_head_nests_beneath_it() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(1, "main", "base-work"),
            pr(2, "base-work", "next"),
            pr(3, "next", "top"),
        ],
        &none(),
    );
    assert_eq!(outline(&tree), ["main (3)", "  #1", "    #2", "      #3"]);
}

#[test]
fn a_branch_point_in_a_stack_carries_several_children() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(1, "main", "root"),
            pr(4, "root", "left"),
            pr(2, "root", "right"),
            pr(7, "left", "leaf"),
        ],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        ["main (4)", "  #1", "    #2", "    #4", "      #7"]
    );
}

#[test]
fn a_stack_on_a_non_default_trunk_stays_under_that_trunk() {
    let tree = build_tree(
        &trunks("main", &["staging"]),
        &[pr(5, "staging", "s1"), pr(6, "s1", "s2")],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        ["main (0)", "staging (2)", "  #5", "    #6"]
    );
}

/// A fork's `main` is a pull request head named like a trunk. The trunk wins,
/// so pull requests based on `main` are not mistaken for a stack on the fork.
#[test]
fn a_trunk_name_beats_a_pull_request_head_of_the_same_name() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[pr(1, "main", "main"), pr(2, "main", "feature")],
        &none(),
    );
    assert_eq!(outline(&tree), ["main (2)", "  #1", "  #2"]);
}

// --- other bases --------------------------------------------------------------

#[test]
fn an_unknown_base_goes_to_other_bases_grouped_and_sorted_by_name() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(4, "zeta", "a"),
            pr(2, "alpha", "b"),
            pr(3, "zeta", "c"),
            pr(1, "main", "d"),
        ],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        [
            "main (1)",
            "  #1",
            "Other bases",
            "? alpha (1)",
            "  #2",
            "? zeta (2)",
            "  #3",
            "  #4",
        ]
    );
}

#[test]
fn a_stack_on_an_unknown_base_nests_inside_its_group() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[pr(1, "legacy", "x"), pr(2, "x", "y")],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        ["main (0)", "Other bases", "? legacy (2)", "  #1", "    #2"]
    );
}

/// Two forks offering a branch called `shared`: a pull request based on
/// `shared` could stack on either, so it is not guessed.
#[test]
fn a_base_shared_by_two_heads_is_ambiguous_and_not_guessed() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(1, "main", "shared"),
            pr(2, "main", "shared"),
            pr(3, "shared", "mine"),
        ],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        [
            "main (2)",
            "  #1",
            "  #2",
            "Other bases",
            "? shared (1)",
            "  #3 [ambiguous]",
        ]
    );
}

/// A pull request whose base and head share a name — a fork's `feature`
/// into the upstream's `feature` — is not its own parent.
#[test]
fn a_pull_request_based_on_its_own_head_name_is_not_its_own_parent() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[pr(1, "feature", "feature")],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        ["main (0)", "Other bases", "? feature (1)", "  #1"]
    );
}

#[test]
fn no_other_bases_heading_when_every_base_is_known() {
    let tree = build_tree(&trunks("main", &[]), &[pr(1, "main", "a")], &none());
    assert!(tree.other_bases.is_empty());
    assert!(!tree.rows().contains(&BranchRow::OtherBases));
}

/// A configured trunk GitHub does not have still claims the pull requests
/// based on it: the base *name* is what places them, and they are not lost
/// to "Other bases" because of a missing branch.
#[test]
fn a_missing_trunk_still_holds_the_pull_requests_based_on_it() {
    let trunks = Trunks::resolve(
        name("main"),
        &TrunkChoice::Configured(vec![name("qa")]),
        &BTreeSet::new(),
    );
    let tree = build_tree(&trunks, &[pr(1, "qa", "x")], &none());
    assert_eq!(tree.trunks[1].drift, TrunkDrift::Missing);
    assert_eq!(outline(&tree), ["main (0)", "qa (1)", "  #1"]);
}

// --- cycles -----------------------------------------------------------------------

#[test]
fn a_two_way_loop_is_cut_at_its_lowest_number() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[pr(8, "a", "b"), pr(5, "b", "a")],
        &none(),
    );
    // #5's base is `b`, #8's head; #8's base is `a`, #5's head.
    assert_eq!(
        outline(&tree),
        [
            "main (0)",
            "Other bases",
            "? b (2)",
            "  #5 [cycle]",
            "    #8"
        ]
    );
    assert_eq!(tree.pull_count(), 2);
}

#[test]
fn a_longer_loop_and_its_hangers_on_each_appear_exactly_once() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(3, "c", "a"),
            pr(1, "a", "b"),
            pr(2, "b", "c"),
            // Stacked on the loop from outside it.
            pr(9, "b", "tail"),
        ],
        &none(),
    );
    assert_eq!(tree.pull_count(), 4);
    let rows = tree.rows();
    for number in [1, 2, 3, 9] {
        let appearances = rows
            .iter()
            .filter(|row| matches!(row, BranchRow::Pull { number: n, .. } if n.0 == number))
            .count();
        assert_eq!(appearances, 1, "#{number}");
    }
    assert_eq!(
        outline(&tree),
        [
            "main (0)",
            "Other bases",
            "? a (4)",
            "  #1 [cycle]",
            "    #2",
            "      #3",
            "    #9",
        ]
    );
}

#[test]
fn two_separate_loops_are_each_cut_once() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(1, "y", "x"),
            pr(2, "x", "y"),
            pr(10, "q", "p"),
            pr(11, "p", "q"),
        ],
        &none(),
    );
    let cuts = tree
        .rows()
        .into_iter()
        .filter(|row| {
            matches!(
                row,
                BranchRow::Pull {
                    note: Some(PullNote::BreaksCycle),
                    ..
                }
            )
        })
        .count();
    assert_eq!(cuts, 2);
    assert_eq!(tree.pull_count(), 4);
}

/// A loop that a trunk-rooted stack points into is still a loop: nothing in
/// it reaches the trunk, and the trunk's own stack is unaffected.
#[test]
fn a_loop_beside_a_healthy_stack_leaves_the_stack_alone() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[
            pr(1, "main", "ok"),
            pr(2, "ok", "ok2"),
            pr(3, "m", "n"),
            pr(4, "n", "m"),
        ],
        &none(),
    );
    assert_eq!(
        outline(&tree),
        [
            "main (2)",
            "  #1",
            "    #2",
            "Other bases",
            "? m (2)",
            "  #3 [cycle]",
            "    #4",
        ]
    );
}

// --- counts ---------------------------------------------------------------------

#[test]
fn the_default_trunk_has_no_distance_and_others_report_theirs() {
    let trunks = trunks("main", &["staging", "develop"]);
    let plan = ComparePlan::new(&trunks, &[]);
    let counts = plan
        .answer(vec![Some(Divergence::new(3, 1)), None])
        .expect("aligned");
    let tree = build_tree(&trunks, &[], &counts);
    let drifts: Vec<TrunkDrift> = tree.trunks.iter().map(|t| t.drift).collect();
    assert_eq!(
        drifts,
        [
            TrunkDrift::Default,
            TrunkDrift::Known(Divergence::new(3, 1)),
            TrunkDrift::Unknown,
        ]
    );
}

#[test]
fn pull_counts_prefer_the_branch_batch_then_fall_back_to_the_feed() {
    let trunks = trunks("main", &[]);
    let mut from_feed = pr(1, "main", "a");
    from_feed.base_divergence = Some(Divergence::new(1, 9));
    let mut both = pr(2, "main", "b");
    both.base_divergence = Some(Divergence::new(0, 0));
    let neither = pr(3, "main", "c");

    let prs = [from_feed, both, neither];
    let plan = ComparePlan::new(&trunks, &prs);
    let counts = plan
        .answer(vec![None, Some(Divergence::new(5, 2)), None])
        .expect("aligned");
    let tree = build_tree(&trunks, &prs, &counts);

    let drifts: Vec<Option<Divergence>> = tree.trunks[0].pulls.iter().map(|p| p.drift).collect();
    assert_eq!(
        drifts,
        [
            Some(Divergence::new(1, 9)),
            Some(Divergence::new(5, 2)),
            None,
        ]
    );
}

#[test]
fn rows_carry_branch_names_and_depths() {
    let tree = build_tree(
        &trunks("main", &[]),
        &[pr(1, "main", "a"), pr(2, "a", "b")],
        &none(),
    );
    let rows = tree.rows();
    assert_eq!(
        rows[2],
        BranchRow::Pull {
            depth: 2,
            number: PrNumber(2),
            head: "b".into(),
            base: "a".into(),
            drift: None,
            note: None,
        }
    );
}

#[test]
fn every_pull_request_appears_exactly_once_in_a_mixed_repository() {
    let prs = [
        pr(1, "main", "a"),
        pr(2, "a", "b"),
        pr(3, "staging", "c"),
        pr(4, "gone", "d"),
        pr(5, "e", "f"),
        pr(6, "f", "e"),
        pr(7, "main", "dup"),
        pr(8, "main", "dup"),
        pr(9, "dup", "g"),
    ];
    let tree = build_tree(&trunks("main", &["staging"]), &prs, &none());
    assert_eq!(tree.pull_count(), prs.len());
}
