//! Adding pull requests to the top of a stack GitHub already has.
//!
//! Two pure questions, answered here so every client asks them the same way:
//!
//! - **Is this a valid extension?** [`plan_extend`]: the stack exists and its
//!   top member is open (its branch is what the additions build on), and the
//!   additions are open same-repository pull requests, listed once, in no
//!   stack already, with heads that are not the stack's own branches.
//! - **Does it need a rewrite?** [`ExtendPlan::needs_rewrite`]: the additions
//!   already chain off the top (the first's base is the top member's head,
//!   each next one's base the previous one's head) and can simply be linked,
//!   or some must be rebased onto the one below — and, because a rebased
//!   branch moves, every addition above it too ([`ExtendPlan::rewrites`]).
//!
//! The stack's existing members are never part of an extension's rewrite.
//!
//! [`continuations`] finds pull requests that already chain past a stack's
//! top, for the feed to offer "Extend stack N with …".

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{
    model::{PrNumber, PullRequest, RepoId},
    state::RepoState,
};

use super::{
    model::{RefName, StackError, StackMembers, StackNumber},
    plan::PlanMember,
};

/// A validated extension of stack `stack` with `additions`, bottom first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendPlan {
    pub repo: RepoId,
    pub stack: StackNumber,
    pub trunk: RefName,
    /// The stack's current top member, which the first addition builds on.
    pub top: PlanMember,
    /// The stack's current members, bottom first; left untouched.
    pub existing: StackMembers,
    additions: Vec<PlanMember>,
    numbers: StackMembers,
}

impl ExtendPlan {
    pub fn additions(&self) -> &[PlanMember] {
        &self.additions
    }

    /// The additions' numbers, validated non-empty and unique.
    pub fn addition_numbers(&self) -> &StackMembers {
        &self.numbers
    }

    /// What addition `ix`'s base must be: the top member's head for the
    /// first, the previous addition's head for the rest.
    pub fn parent_of(&self, ix: usize) -> &RefName {
        match ix {
            0 => &self.top.head,
            _ => &self.additions[ix - 1].head,
        }
    }

    /// Whether addition `ix` already targets what it must.
    pub fn is_chained(&self, ix: usize) -> bool {
        self.additions
            .get(ix)
            .is_some_and(|member| &member.base == self.parent_of(ix))
    }

    /// Whether any addition must be rebased and force-pushed. `false` means
    /// the extension is a plain `gh stack link`.
    pub fn needs_rewrite(&self) -> bool {
        (0..self.additions.len()).any(|ix| !self.is_chained(ix))
    }

    /// The additions whose branches the extension will rewrite: the first
    /// one not already chained, and every addition above it (their parent
    /// moves when it is rebased). Empty when nothing needs a rewrite.
    pub fn rewrites(&self) -> &[PlanMember] {
        let first = (0..self.additions.len())
            .find(|ix| !self.is_chained(*ix))
            .unwrap_or(self.additions.len());
        &self.additions[first..]
    }
}

/// Why pull requests cannot be added to a stack.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExtendError {
    #[error("stack {0} is not one of this repository's open stacks")]
    UnknownStack(StackNumber),
    #[error("pick at least one pull request to add")]
    NothingToAdd,
    #[error("the top of stack {stack}, {top}, is not open; a stack can only grow from an open top")]
    TopNotOpen { stack: StackNumber, top: PrNumber },
    #[error("{0} is not an open pull request in this repository")]
    NotOpen(PrNumber),
    #[error("{0} comes from a fork; its branch cannot be rebased or stacked from here")]
    Fork(PrNumber),
    #[error("{0} is listed twice")]
    Duplicate(PrNumber),
    #[error("{0} is already in this stack")]
    AlreadyInStack(PrNumber),
    #[error("{number} is in stack {other}; unstack it first")]
    InOtherStack {
        number: PrNumber,
        other: StackNumber,
    },
    #[error("{number}'s head `{head}` is one of the stack's own branches")]
    StackBranch { number: PrNumber, head: String },
    #[error("{first} and {second} share the head branch `{head}`")]
    SharedHead {
        head: String,
        first: PrNumber,
        second: PrNumber,
    },
    #[error(transparent)]
    Ref(#[from] StackError),
}

fn member(pr: &PullRequest) -> Result<PlanMember, StackError> {
    Ok(PlanMember {
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        head: RefName::new(pr.head_ref.as_str())?,
        base: RefName::new(pr.base_ref.as_str())?,
    })
}

/// Validate adding `order` (bottom first) to the top of stack `stack`.
pub fn plan_extend(
    repo: &RepoState,
    stack: StackNumber,
    order: &[PrNumber],
) -> Result<ExtendPlan, ExtendError> {
    let target = repo
        .stacks
        .iter()
        .find(|s| s.repo == repo.id && s.number == Some(stack))
        .ok_or(ExtendError::UnknownStack(stack))?;
    if order.is_empty() {
        return Err(ExtendError::NothingToAdd);
    }
    let open = |number: PrNumber| repo.prs.iter().find(|pr| pr.number == number);

    let top_number = target.members.top();
    let top_pr = open(top_number).ok_or(ExtendError::TopNotOpen {
        stack,
        top: top_number,
    })?;
    let top = member(top_pr)?;

    // Branches the stack already owns: its trunk and every open member's head.
    let mut stack_branches: BTreeSet<&str> = BTreeSet::from([target.trunk.as_str()]);
    stack_branches.extend(
        target
            .members
            .as_slice()
            .iter()
            .filter_map(|n| open(*n))
            .map(|pr| pr.head_ref.as_str()),
    );
    let in_other: BTreeMap<PrNumber, StackNumber> = repo
        .stacks
        .iter()
        .filter(|s| s.repo == repo.id && s.number != Some(stack))
        .filter_map(|s| s.number.map(|n| (s, n)))
        .flat_map(|(s, n)| s.members.as_slice().iter().map(move |m| (*m, n)))
        .collect();

    let mut seen = BTreeSet::new();
    let mut heads: BTreeMap<String, PrNumber> = BTreeMap::new();
    let mut additions = Vec::with_capacity(order.len());
    for number in order {
        if !seen.insert(*number) {
            return Err(ExtendError::Duplicate(*number));
        }
        if target.members.contains(*number) {
            return Err(ExtendError::AlreadyInStack(*number));
        }
        if let Some(other) = in_other.get(number) {
            return Err(ExtendError::InOtherStack {
                number: *number,
                other: *other,
            });
        }
        let pr = open(*number).ok_or(ExtendError::NotOpen(*number))?;
        if pr.is_cross_repository {
            return Err(ExtendError::Fork(*number));
        }
        if stack_branches.contains(pr.head_ref.as_str()) {
            return Err(ExtendError::StackBranch {
                number: *number,
                head: pr.head_ref.clone(),
            });
        }
        if let Some(first) = heads.insert(pr.head_ref.clone(), *number) {
            return Err(ExtendError::SharedHead {
                head: pr.head_ref.clone(),
                first,
                second: *number,
            });
        }
        additions.push(member(pr)?);
    }

    let numbers = StackMembers::new(additions.iter().map(|m| m.number).collect())?;
    Ok(ExtendPlan {
        repo: repo.id.clone(),
        stack,
        trunk: target.trunk.clone(),
        top,
        existing: target.members.clone(),
        additions,
        numbers,
    })
}

/// Open pull requests that already chain past the top of one of GitHub's
/// stacks, bottom first: what "Extend stack N with …" offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Continuation {
    pub stack: StackNumber,
    pub additions: StackMembers,
}

/// For each of the repository's GitHub stacks with an open top, the line of
/// pull requests built on that top, under the same rules as
/// [`super::detect_chains`]: same-repository only, in no stack, a head no
/// other open pull request shares, and stopping where the line branches.
pub fn continuations(repo: &RepoState) -> Vec<Continuation> {
    let prs = &repo.prs;
    let claimed: BTreeSet<PrNumber> = repo
        .stacks
        .iter()
        .flat_map(|s| s.members.as_slice().iter().copied())
        .collect();
    let mut heads: HashMap<&str, usize> = HashMap::new();
    for pr in prs.iter().filter(|pr| !pr.is_cross_repository) {
        *heads.entry(pr.head_ref.as_str()).or_default() += 1;
    }
    let candidate = |pr: &PullRequest| {
        !pr.is_cross_repository
            && !claimed.contains(&pr.number)
            && pr.head_ref != pr.base_ref
            && RefName::new(pr.head_ref.as_str()).is_ok()
            && RefName::new(pr.base_ref.as_str()).is_ok()
    };
    // The one candidate built on `head`, if exactly one is and `head` names
    // exactly one open pull request's branch.
    let only_child = |head: &str| -> Option<&PullRequest> {
        if heads.get(head).copied() != Some(1) {
            return None;
        }
        let mut children = prs.iter().filter(|pr| candidate(pr) && pr.base_ref == head);
        match (children.next(), children.next()) {
            (Some(child), None) => Some(child),
            _ => None,
        }
    };

    let mut out = Vec::new();
    for stack in repo.stacks.iter().filter(|s| s.repo == repo.id) {
        let Some(number) = stack.number else {
            continue;
        };
        let Some(top) = prs
            .iter()
            .find(|pr| pr.number == stack.members.top() && !pr.is_cross_repository)
        else {
            continue;
        };
        let mut line = Vec::new();
        let mut visited = BTreeSet::new();
        let mut at = top.head_ref.as_str();
        while let Some(child) = only_child(at) {
            if !visited.insert(child.number) {
                break;
            }
            line.push(child.number);
            at = child.head_ref.as_str();
        }
        if let Ok(additions) = StackMembers::new(line) {
            out.push(Continuation {
                stack: number,
                additions,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{stack::model::Stack, state::LoadState, test_support::pull};

    fn link(number: u32, head: &str, base: &str) -> PullRequest {
        let mut pr = pull(number);
        pr.head_ref = head.into();
        pr.base_ref = base.into();
        pr
    }

    fn stack(number: u32, members: &[u32]) -> Stack {
        Stack {
            repo: RepoId::new("o", "r"),
            number: StackNumber::new(number),
            trunk: RefName::new("main").expect("valid"),
            members: StackMembers::new(members.iter().copied().map(PrNumber).collect())
                .expect("valid"),
        }
    }

    fn repo(prs: Vec<PullRequest>, stacks: Vec<Stack>) -> RepoState {
        RepoState {
            prs,
            stacks,
            load: LoadState::Idle,
            ..RepoState::new(RepoId::new("o", "r"))
        }
    }

    fn n(numbers: &[u32]) -> Vec<PrNumber> {
        numbers.iter().copied().map(PrNumber).collect()
    }

    fn seven() -> StackNumber {
        StackNumber::new(7).expect("non-zero")
    }

    /// Stack 7 is #1 (a, on main) ← #2 (b, on a).
    fn base_prs() -> Vec<PullRequest> {
        vec![link(1, "a", "main"), link(2, "b", "a")]
    }

    fn with(extra: Vec<PullRequest>) -> RepoState {
        let mut prs = base_prs();
        prs.extend(extra);
        repo(prs, vec![stack(7, &[1, 2])])
    }

    #[test]
    fn additions_that_chain_off_the_top_need_no_rewrite() {
        let state = with(vec![link(3, "c", "b"), link(4, "d", "c")]);
        let plan = plan_extend(&state, seven(), &n(&[3, 4])).expect("valid");
        assert!(!plan.needs_rewrite());
        assert!(plan.rewrites().is_empty());
        assert_eq!(plan.parent_of(0).as_str(), "b");
        assert_eq!(plan.parent_of(1).as_str(), "c");
        assert_eq!(plan.top.number, PrNumber(2));
        assert_eq!(plan.addition_numbers().as_slice(), &n(&[3, 4])[..]);
        assert_eq!(plan.existing.as_slice(), &n(&[1, 2])[..]);
        assert_eq!(plan.trunk.as_str(), "main");
    }

    #[test]
    fn an_unchained_addition_and_everything_above_it_is_rewritten() {
        // #3 chains; #4 targets main; #5 chains off #4 but #4 will move.
        let state = with(vec![
            link(3, "c", "b"),
            link(4, "d", "main"),
            link(5, "e", "d"),
        ]);
        let plan = plan_extend(&state, seven(), &n(&[3, 4, 5])).expect("valid");
        assert!(plan.needs_rewrite());
        assert!(plan.is_chained(0));
        assert!(!plan.is_chained(1));
        assert!(plan.is_chained(2));
        let rewritten: Vec<u32> = plan.rewrites().iter().map(|m| m.number.0).collect();
        assert_eq!(rewritten, vec![4, 5]);
    }

    #[test]
    fn the_order_chosen_decides_chaining() {
        let state = with(vec![link(3, "c", "b"), link(4, "d", "c")]);
        let plan = plan_extend(&state, seven(), &n(&[4, 3])).expect("valid");
        assert!(plan.needs_rewrite());
        let rewritten: Vec<u32> = plan.rewrites().iter().map(|m| m.number.0).collect();
        assert_eq!(rewritten, vec![4, 3]);
    }

    #[test]
    fn every_invalid_request_is_a_typed_error() {
        let mut fork = link(6, "f", "b");
        fork.is_cross_repository = true;
        let mut state = with(vec![
            link(3, "c", "b"),
            link(4, "c", "main"),
            fork,
            link(8, "x", "main"),
            link(9, "y", "x"),
            link(10, "main", "b"),
            link(11, "a", "b"),
        ]);
        state.stacks.push(stack(8, &[8, 9]));

        assert_eq!(
            plan_extend(&state, StackNumber::new(99).expect("non-zero"), &n(&[3])),
            Err(ExtendError::UnknownStack(
                StackNumber::new(99).expect("non-zero")
            ))
        );
        assert_eq!(
            plan_extend(&state, seven(), &[]),
            Err(ExtendError::NothingToAdd)
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[42])),
            Err(ExtendError::NotOpen(PrNumber(42)))
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[6])),
            Err(ExtendError::Fork(PrNumber(6)))
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[3, 3])),
            Err(ExtendError::Duplicate(PrNumber(3)))
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[2])),
            Err(ExtendError::AlreadyInStack(PrNumber(2)))
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[9])),
            Err(ExtendError::InOtherStack {
                number: PrNumber(9),
                other: StackNumber::new(8).expect("non-zero"),
            })
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[3, 4])),
            Err(ExtendError::SharedHead {
                head: "c".into(),
                first: PrNumber(3),
                second: PrNumber(4),
            })
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[10])),
            Err(ExtendError::StackBranch {
                number: PrNumber(10),
                head: "main".into(),
            })
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[11])),
            Err(ExtendError::StackBranch {
                number: PrNumber(11),
                head: "a".into(),
            })
        );
    }

    #[test]
    fn a_stack_whose_top_has_merged_cannot_grow() {
        // #2 merged: only #1 is open.
        let state = repo(
            vec![link(1, "a", "main"), link(3, "c", "b")],
            vec![stack(7, &[1, 2])],
        );
        assert_eq!(
            plan_extend(&state, seven(), &n(&[3])),
            Err(ExtendError::TopNotOpen {
                stack: seven(),
                top: PrNumber(2),
            })
        );
    }

    #[test]
    fn a_detected_chain_is_not_an_existing_stack() {
        let state = repo(base_prs(), vec![]);
        assert_eq!(
            plan_extend(&state, seven(), &n(&[2])),
            Err(ExtendError::UnknownStack(seven()))
        );
    }

    #[test]
    fn a_continuation_past_the_top_is_found() {
        let state = with(vec![
            link(3, "c", "b"),
            link(4, "d", "c"),
            link(5, "z", "main"),
        ]);
        assert_eq!(
            continuations(&state),
            vec![Continuation {
                stack: seven(),
                additions: StackMembers::new(n(&[3, 4])).expect("valid"),
            }]
        );
    }

    #[test]
    fn a_continuation_stops_where_the_line_branches_or_is_ambiguous() {
        // Two pull requests on the top: no continuation at all.
        let state = with(vec![link(3, "c", "b"), link(4, "d", "b")]);
        assert!(continuations(&state).is_empty());

        // #3 on the top, then two on #3: just #3.
        let state = with(vec![
            link(3, "c", "b"),
            link(4, "d", "c"),
            link(5, "e", "c"),
        ]);
        assert_eq!(continuations(&state)[0].additions.as_slice(), &n(&[3])[..]);

        // The top's head is shared by another open pull request: ambiguous.
        let state = with(vec![link(3, "b", "develop"), link(4, "d", "b")]);
        assert!(continuations(&state).is_empty());
    }

    #[test]
    fn continuations_skip_forks_other_stacks_and_merged_tops() {
        let mut fork = link(3, "c", "b");
        fork.is_cross_repository = true;
        assert!(continuations(&with(vec![fork])).is_empty());

        let mut state = with(vec![link(3, "c", "b")]);
        state.stacks.push(stack(9, &[3]));
        assert!(continuations(&state).is_empty());

        let state = repo(
            vec![link(1, "a", "main"), link(3, "c", "b")],
            vec![stack(7, &[1, 2])],
        );
        assert!(continuations(&state).is_empty());
    }

    #[test]
    fn a_cycle_hanging_off_the_top_is_not_followed_forever() {
        // #3 on the top; #4 and #5 form a cycle with no way in from #3.
        let state = with(vec![
            link(3, "c", "b"),
            link(4, "p", "q"),
            link(5, "q", "p"),
        ]);
        assert_eq!(continuations(&state)[0].additions.as_slice(), &n(&[3])[..]);
    }
}
