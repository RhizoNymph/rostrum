//! Validating a request to make pull requests into a stack.
//!
//! Both ways of making a stack end in the same plan: "Make stack" on a
//! detected chain (whose bases already chain, so nothing is rewritten) and
//! "Arrange" on pull requests the user picked and ordered (whose branches
//! may have to be rebased onto each other). Everything that can be checked
//! without touching git or GitHub is checked here, so a plan in hand is one
//! the executor can start on.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    model::{PrNumber, RepoId},
    state::RepoState,
};

use super::model::{RefName, Stack, StackError, StackMembers, StackNumber};

/// One pull request in a plan, with what the executor needs to know about it
/// as of when the plan was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanMember {
    pub number: PrNumber,
    pub title: String,
    pub url: String,
    pub head: RefName,
    /// The base the pull request targets on GitHub now.
    pub base: RefName,
}

/// A validated stack-to-be: two or more same-repository pull requests,
/// bottom first, over a trunk none of them heads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackPlan {
    pub repo: RepoId,
    pub trunk: RefName,
    members: Vec<PlanMember>,
    /// The same numbers, kept in their validated form so [`Self::as_stack`]
    /// cannot fail.
    numbers: StackMembers,
}

impl StackPlan {
    pub fn members(&self) -> &[PlanMember] {
        &self.members
    }

    /// What member `ix`'s base must be once it is stacked: the trunk for the
    /// bottom, the head of the member below for the rest.
    pub fn parent_of(&self, ix: usize) -> &RefName {
        match ix {
            0 => &self.trunk,
            _ => &self.members[ix - 1].head,
        }
    }

    /// Whether any member must be rebased and force-pushed, because its base
    /// is not already what the stack needs it to be.
    ///
    /// A plan that needs no rewrite is "Make stack": `gh stack` adopts the
    /// branches as they are.
    pub fn needs_rewrite(&self) -> bool {
        self.members
            .iter()
            .enumerate()
            .any(|(ix, member)| &member.base != self.parent_of(ix))
    }

    /// The stack this plan describes, before GitHub has numbered it.
    pub fn as_stack(&self) -> Stack {
        Stack {
            repo: self.repo.clone(),
            number: None,
            trunk: self.trunk.clone(),
            members: self.numbers.clone(),
        }
    }
}

/// Why a set of pull requests cannot be made into a stack.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("a stack needs at least two pull requests")]
    TooFew,
    #[error("{0} is not an open pull request in this repository")]
    NotOpen(PrNumber),
    #[error("{0} comes from a fork; its branch cannot be rebased or stacked from here")]
    Fork(PrNumber),
    #[error("{0} is listed twice")]
    Duplicate(PrNumber),
    #[error("{first} and {second} share the head branch `{head}`")]
    SharedHead {
        head: String,
        first: PrNumber,
        second: PrNumber,
    },
    #[error("the trunk `{trunk}` is the head branch of {number}")]
    TrunkIsMember { trunk: String, number: PrNumber },
    #[error("{number} is already in stack {stack} on GitHub; unstack it first")]
    AlreadyStacked {
        number: PrNumber,
        stack: StackNumber,
    },
    #[error(transparent)]
    Ref(#[from] StackError),
}

/// Validate `order` (bottom first) over `trunk` against what `repo` knows.
pub fn plan_stack(
    repo: &RepoState,
    order: &[PrNumber],
    trunk: RefName,
) -> Result<StackPlan, PlanError> {
    if order.len() < 2 {
        return Err(PlanError::TooFew);
    }

    let stacked: BTreeMap<PrNumber, StackNumber> = repo
        .stacks
        .iter()
        .filter_map(|stack| stack.number.map(|n| (stack, n)))
        .flat_map(|(stack, n)| stack.members.as_slice().iter().map(move |m| (*m, n)))
        .collect();

    let mut seen = BTreeSet::new();
    let mut heads: BTreeMap<String, PrNumber> = BTreeMap::new();
    let mut members = Vec::with_capacity(order.len());
    for number in order {
        if !seen.insert(*number) {
            return Err(PlanError::Duplicate(*number));
        }
        let pr = repo
            .prs
            .iter()
            .find(|pr| pr.number == *number)
            .ok_or(PlanError::NotOpen(*number))?;
        if pr.is_cross_repository {
            return Err(PlanError::Fork(*number));
        }
        if let Some(stack) = stacked.get(number) {
            return Err(PlanError::AlreadyStacked {
                number: *number,
                stack: *stack,
            });
        }
        if let Some(first) = heads.insert(pr.head_ref.clone(), *number) {
            return Err(PlanError::SharedHead {
                head: pr.head_ref.clone(),
                first,
                second: *number,
            });
        }
        if pr.head_ref == trunk.as_str() {
            return Err(PlanError::TrunkIsMember {
                trunk: trunk.to_string(),
                number: *number,
            });
        }
        members.push(PlanMember {
            number: *number,
            title: pr.title.clone(),
            url: pr.url.clone(),
            head: RefName::new(pr.head_ref.as_str())?,
            base: RefName::new(pr.base_ref.as_str())?,
        });
    }

    let numbers = StackMembers::new(members.iter().map(|m| m.number).collect())?;
    Ok(StackPlan {
        repo: repo.id.clone(),
        trunk,
        members,
        numbers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::PullRequest, state::LoadState, test_support::pull};

    fn link(number: u32, head: &str, base: &str) -> PullRequest {
        let mut pr = pull(number);
        pr.head_ref = head.into();
        pr.base_ref = base.into();
        pr
    }

    fn repo(prs: Vec<PullRequest>) -> RepoState {
        RepoState {
            id: RepoId::new("o", "r"),
            prs,
            load: LoadState::Idle,
            collapsed: false,
            stacks: Vec::new(),
            meta: None,
        }
    }

    fn main() -> RefName {
        RefName::new("main").expect("valid")
    }

    fn order(numbers: &[u32]) -> Vec<PrNumber> {
        numbers.iter().copied().map(PrNumber).collect()
    }

    #[test]
    fn an_existing_chain_needs_no_rewrite() {
        let state = repo(vec![link(1, "a", "main"), link(2, "b", "a")]);
        let plan = plan_stack(&state, &order(&[1, 2]), main()).expect("valid");
        assert!(!plan.needs_rewrite());
        assert_eq!(plan.parent_of(0).as_str(), "main");
        assert_eq!(plan.parent_of(1).as_str(), "a");
        assert_eq!(plan.as_stack().members.as_slice(), &order(&[1, 2])[..]);
    }

    #[test]
    fn independent_pull_requests_need_a_rewrite() {
        let state = repo(vec![link(1, "a", "main"), link(2, "b", "main")]);
        let plan = plan_stack(&state, &order(&[1, 2]), main()).expect("valid");
        assert!(plan.needs_rewrite());
    }

    #[test]
    fn reordering_a_chain_needs_a_rewrite() {
        let state = repo(vec![link(1, "a", "main"), link(2, "b", "a")]);
        let plan = plan_stack(&state, &order(&[2, 1]), main()).expect("valid");
        assert!(plan.needs_rewrite());
        assert_eq!(plan.parent_of(1).as_str(), "b");
    }

    #[test]
    fn a_different_trunk_needs_a_rewrite_of_the_bottom() {
        let state = repo(vec![link(1, "a", "main"), link(2, "b", "a")]);
        let develop = RefName::new("develop").expect("valid");
        assert!(
            plan_stack(&state, &order(&[1, 2]), develop)
                .expect("valid")
                .needs_rewrite()
        );
    }

    #[test]
    fn every_invalid_request_is_named() {
        let mut forked = link(3, "c", "main");
        forked.is_cross_repository = true;
        let state = repo(vec![
            link(1, "a", "main"),
            link(2, "b", "main"),
            forked,
            link(4, "a", "develop"),
        ]);

        assert_eq!(
            plan_stack(&state, &order(&[1]), main()),
            Err(PlanError::TooFew)
        );
        assert_eq!(
            plan_stack(&state, &order(&[]), main()),
            Err(PlanError::TooFew)
        );
        assert_eq!(
            plan_stack(&state, &order(&[1, 9]), main()),
            Err(PlanError::NotOpen(PrNumber(9)))
        );
        assert_eq!(
            plan_stack(&state, &order(&[1, 3]), main()),
            Err(PlanError::Fork(PrNumber(3)))
        );
        assert_eq!(
            plan_stack(&state, &order(&[1, 2, 1]), main()),
            Err(PlanError::Duplicate(PrNumber(1)))
        );
        assert_eq!(
            plan_stack(&state, &order(&[1, 4]), main()),
            Err(PlanError::SharedHead {
                head: "a".into(),
                first: PrNumber(1),
                second: PrNumber(4),
            })
        );
        assert_eq!(
            plan_stack(&state, &order(&[1, 2]), RefName::new("b").expect("valid")),
            Err(PlanError::TrunkIsMember {
                trunk: "b".into(),
                number: PrNumber(2),
            })
        );
    }

    #[test]
    fn members_of_a_github_stack_must_be_unstacked_first() {
        let mut state = repo(vec![
            link(1, "a", "main"),
            link(2, "b", "a"),
            link(3, "c", "main"),
        ]);
        state.stacks.push(Stack {
            repo: state.id.clone(),
            number: StackNumber::new(7),
            trunk: main(),
            members: StackMembers::new(order(&[1, 2])).expect("valid"),
        });
        assert_eq!(
            plan_stack(&state, &order(&[3, 2]), main()),
            Err(PlanError::AlreadyStacked {
                number: PrNumber(2),
                stack: StackNumber::new(7).expect("non-zero"),
            })
        );
    }

    #[test]
    fn an_unusable_branch_name_is_reported() {
        let state = repo(vec![link(1, "a", "main"), link(2, "has space", "main")]);
        assert!(matches!(
            plan_stack(&state, &order(&[1, 2]), main()),
            Err(PlanError::Ref(_))
        ));
    }
}
