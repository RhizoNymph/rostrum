//! Finding chains of pull requests that could be a stack.
//!
//! A chain is read from base and head branch names alone: the bottom pull
//! request targets a branch that is no open pull request's head (the trunk),
//! and each one above targets the head of the one below. Everything here is
//! defensive, because the input is whatever people happened to open:
//!
//! - **Forks** are excluded. A fork's head names a branch in another
//!   repository, so a same-repository pull request whose base shares that
//!   name is not built on it.
//! - **Ambiguity** breaks a chain. Two open pull requests with the same head
//!   branch (GitHub allows it, against different bases) make "the pull request
//!   below" unknowable, so nothing chains onto either.
//! - **Branching** stops a chain. A stack is a line; when two pull requests
//!   target the same head, the chain ends at that head and neither branch is
//!   guessed at.
//! - **Cycles** are never entered. A chain is walked up from a pull request
//!   whose base is not a pull request, and every node in a cycle has its
//!   parent inside the cycle, so no walk reaches one. A visited set backs that
//!   up regardless.
//! - **Claimed** pull requests — already in a stack GitHub knows — neither
//!   start nor join a detected chain, and a pull request built on one is not a
//!   chain bottom either: its "trunk" would be another stack's member.

use std::collections::{BTreeSet, HashMap};

use crate::model::{PrNumber, PullRequest, RepoId};

use super::model::{RefName, Stack, StackMembers};

/// Where a candidate's base points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parent {
    /// A branch that is no open same-repository pull request's head.
    Trunk,
    /// Exactly one candidate's head.
    Pr(usize),
    /// Something that rules the pull request out of any chain: more than one
    /// pull request's head, or the head of one that is not a candidate.
    Unusable,
}

/// Every maximal chain of two or more pull requests in `prs`, in the order of
/// their bottom pull request in `prs`.
///
/// `claimed` are pull requests already in a stack GitHub knows about.
pub fn detect_chains(
    repo: &RepoId,
    prs: &[PullRequest],
    claimed: &BTreeSet<PrNumber>,
) -> Vec<Stack> {
    // Same-repository pull requests, indexed by head branch. Built over every
    // one of them, not just candidates, so a base that names a claimed pull
    // request's head is recognised as such rather than taken for a trunk.
    let mut heads: HashMap<&str, Vec<usize>> = HashMap::new();
    for (ix, pr) in prs.iter().enumerate() {
        if !pr.is_cross_repository {
            heads.entry(pr.head_ref.as_str()).or_default().push(ix);
        }
    }

    let candidate = |ix: usize| {
        let pr = &prs[ix];
        !pr.is_cross_repository
            && !claimed.contains(&pr.number)
            && pr.head_ref != pr.base_ref
            && RefName::new(pr.head_ref.as_str()).is_ok()
            && RefName::new(pr.base_ref.as_str()).is_ok()
    };

    let parent = |ix: usize| -> Parent {
        match heads.get(prs[ix].base_ref.as_str()).map(Vec::as_slice) {
            None | Some([]) => Parent::Trunk,
            Some([only]) if *only != ix && candidate(*only) => Parent::Pr(*only),
            Some(_) => Parent::Unusable,
        }
    };

    let mut parents = vec![Parent::Unusable; prs.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); prs.len()];
    for ix in (0..prs.len()).filter(|ix| candidate(*ix)) {
        parents[ix] = parent(ix);
        if let Parent::Pr(up) = parents[ix] {
            children[up].push(ix);
        }
    }

    let mut chains = Vec::new();
    for root in (0..prs.len()).filter(|ix| candidate(*ix) && parents[*ix] == Parent::Trunk) {
        let mut chain = vec![root];
        let mut visited = BTreeSet::from([root]);
        let mut at = root;
        while let [only] = children[at].as_slice() {
            if !visited.insert(*only) {
                break;
            }
            chain.push(*only);
            at = *only;
        }
        if chain.len() < 2 {
            continue;
        }

        let Ok(trunk) = RefName::new(prs[root].base_ref.as_str()) else {
            continue;
        };
        let numbers: Vec<PrNumber> = chain.iter().map(|ix| prs[*ix].number).collect();
        // Two entries with one number would be a malformed list from
        // upstream; such a chain is skipped rather than guessed at.
        if let Ok(members) = StackMembers::new(numbers) {
            chains.push(Stack {
                repo: repo.clone(),
                number: None,
                trunk,
                members,
            });
        }
    }
    chains
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::pull as pr;

    fn repo() -> RepoId {
        RepoId::new("o", "r")
    }

    /// A same-repository pull request `number` from `head` into `base`.
    fn link(number: u32, head: &str, base: &str) -> PullRequest {
        let mut pull = pr(number);
        pull.head_ref = head.into();
        pull.base_ref = base.into();
        pull
    }

    fn fork(number: u32, head: &str, base: &str) -> PullRequest {
        let mut pull = link(number, head, base);
        pull.is_cross_repository = true;
        pull
    }

    fn chains(prs: &[PullRequest]) -> Vec<(String, Vec<u32>)> {
        chains_claimed(prs, &[])
    }

    fn chains_claimed(prs: &[PullRequest], claimed: &[u32]) -> Vec<(String, Vec<u32>)> {
        let claimed: BTreeSet<PrNumber> = claimed.iter().copied().map(PrNumber).collect();
        detect_chains(&repo(), prs, &claimed)
            .into_iter()
            .map(|stack| {
                assert_eq!(stack.number, None, "detected chains have no number");
                assert_eq!(stack.repo, repo());
                (
                    stack.trunk.to_string(),
                    stack.members.as_slice().iter().map(|n| n.0).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_simple_chain_is_found_bottom_first() {
        let prs = [link(3, "c", "b"), link(1, "a", "main"), link(2, "b", "a")];
        assert_eq!(chains(&prs), vec![("main".into(), vec![1, 2, 3])]);
    }

    #[test]
    fn independent_pull_requests_are_not_a_chain() {
        let prs = [link(1, "a", "main"), link(2, "b", "main")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn a_single_pull_request_on_a_non_default_trunk_is_not_a_chain() {
        assert!(chains(&[link(1, "a", "develop")]).is_empty());
    }

    #[test]
    fn the_trunk_can_be_any_branch_that_is_not_a_pull_request_head() {
        let prs = [link(1, "a", "release/1.0"), link(2, "b", "a")];
        assert_eq!(chains(&prs), vec![("release/1.0".into(), vec![1, 2])]);
    }

    #[test]
    fn two_separate_chains_are_both_found_in_bottom_order() {
        let prs = [
            link(10, "y1", "main"),
            link(1, "x1", "main"),
            link(11, "y2", "y1"),
            link(2, "x2", "x1"),
        ];
        assert_eq!(
            chains(&prs),
            vec![("main".into(), vec![10, 11]), ("main".into(), vec![1, 2])]
        );
    }

    #[test]
    fn a_fork_head_is_never_a_parent() {
        // The fork's `a` lives in the fork; #2 targets this repository's `a`,
        // which no open pull request here heads, so it is a trunk.
        let prs = [fork(1, "a", "main"), link(2, "b", "a"), link(3, "c", "b")];
        assert_eq!(chains(&prs), vec![("a".into(), vec![2, 3])]);
    }

    #[test]
    fn a_fork_pull_request_never_joins_a_chain() {
        let prs = [link(1, "a", "main"), fork(2, "b", "a")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn a_fork_pull_request_does_not_cut_a_same_repository_chain() {
        let prs = [link(1, "a", "main"), link(2, "b", "a"), fork(3, "c", "a")];
        assert_eq!(chains(&prs), vec![("main".into(), vec![1, 2])]);
    }

    #[test]
    fn a_two_node_cycle_is_ignored() {
        let prs = [link(1, "a", "b"), link(2, "b", "a")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn a_three_node_cycle_is_ignored_and_does_not_hang() {
        let prs = [link(1, "a", "c"), link(2, "b", "a"), link(3, "c", "b")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn a_cycle_beside_a_chain_leaves_the_chain_alone() {
        let prs = [
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(5, "p", "q"),
            link(6, "q", "p"),
        ];
        assert_eq!(chains(&prs), vec![("main".into(), vec![1, 2])]);
    }

    #[test]
    fn a_pull_request_into_itself_is_ignored() {
        let prs = [link(1, "a", "a"), link(2, "b", "a")];
        // #1 is not a candidate; #2's base names #1's head, which is not a
        // candidate, so #2 cannot be a bottom either.
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn branching_ends_the_chain_at_the_fork_point() {
        // a ← b ← {c, d}: the line is a, b; neither c nor d is guessed.
        let prs = [
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(3, "c", "b"),
            link(4, "d", "b"),
        ];
        assert_eq!(chains(&prs), vec![("main".into(), vec![1, 2])]);
    }

    #[test]
    fn branching_at_the_bottom_leaves_nothing_to_group() {
        let prs = [link(1, "a", "main"), link(2, "b", "a"), link(3, "c", "a")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn two_pull_requests_with_one_head_make_that_head_unusable() {
        // #1 and #2 both head `a`; #3 targets `a` and cannot know which.
        let prs = [
            link(1, "a", "main"),
            link(2, "a", "develop"),
            link(3, "b", "a"),
            link(4, "c", "b"),
        ];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn a_duplicated_head_can_still_sit_on_top_of_a_chain() {
        let prs = [
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(3, "b", "develop"),
        ];
        // #2 and #3 share head `b`; #2 itself still chains onto #1.
        assert_eq!(chains(&prs), vec![("main".into(), vec![1, 2])]);
    }

    #[test]
    fn claimed_pull_requests_neither_start_nor_join_a_chain() {
        let prs = [link(1, "a", "main"), link(2, "b", "a"), link(3, "c", "b")];
        assert!(chains_claimed(&prs, &[1, 2, 3]).is_empty());
        // #1 claimed: #2 sits on another stack's member, so it is no bottom.
        assert!(chains_claimed(&prs, &[1]).is_empty());
        // #3 claimed: the rest is still a chain.
        assert_eq!(
            chains_claimed(&prs, &[3]),
            vec![("main".into(), vec![1, 2])]
        );
    }

    #[test]
    fn a_pull_request_on_a_claimed_head_does_not_start_a_chain() {
        let prs = [
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(3, "c", "b"),
            link(4, "d", "c"),
        ];
        assert!(chains_claimed(&prs, &[1, 2]).is_empty());
    }

    #[test]
    fn unusable_branch_names_are_skipped() {
        let prs = [link(1, "a", "main"), link(2, "has space", "a")];
        assert!(chains(&prs).is_empty());
        let prs = [link(1, "-x", "main"), link(2, "b", "-x")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn duplicate_numbers_in_the_input_do_not_produce_a_malformed_stack() {
        let prs = [link(1, "a", "main"), link(1, "b", "a")];
        assert!(chains(&prs).is_empty());
    }

    #[test]
    fn a_long_chain_is_found_whole() {
        let mut prs = vec![link(1, "b1", "main")];
        for n in 2..=40u32 {
            prs.push(link(n, &format!("b{n}"), &format!("b{}", n - 1)));
        }
        prs.reverse();
        let found = chains(&prs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, (1..=40).collect::<Vec<_>>());
    }

    #[test]
    fn drafts_take_part_like_any_other_pull_request() {
        let mut top = link(2, "b", "a");
        top.is_draft = true;
        let prs = [link(1, "a", "main"), top];
        assert_eq!(chains(&prs), vec![("main".into(), vec![1, 2])]);
    }

    #[test]
    fn an_empty_feed_has_no_chains() {
        assert!(chains(&[]).is_empty());
    }
}
