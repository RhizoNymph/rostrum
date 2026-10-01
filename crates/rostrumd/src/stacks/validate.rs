//! Checking a phone's stack request against GitHub as it is now.
//!
//! Pure: every function takes the repository's state (its open pull requests
//! and GitHub's stacks, freshly fetched) and the request, and answers with a
//! plan `rostrum-stack` can run or a typed refusal. The plans themselves are
//! `rostrum-core`'s — the same `plan_stack` and `plan_extend` the desktop's
//! panels use — so the phone and the desktop cannot disagree about what is
//! valid, or about which branches a rewrite touches.

use rostrum_core::{
    ExtendError, ExtendPlan, PlanError, PlanMember, PrNumber, RefName, RepoId, RepoState,
    StackNumber, StackPlan, plan_extend, plan_stack,
};
use rostrum_remote::{RewriteBranch, StackRewritePlan, confirms_exactly};

/// Why a stack request was refused before anything ran.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StackRequestError {
    #[error("no clone of {0} is configured on this computer")]
    NotConfigured(RepoId),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error(transparent)]
    Extend(#[from] ExtendError),
    #[error("stack {stack} is not one of {repo}'s open stacks")]
    UnknownStack { repo: RepoId, stack: StackNumber },
    /// "Make stack" on pull requests whose bases do not already chain.
    #[error(
        "these pull requests do not chain in this order; arranging them would rewrite {}",
        names(.rewrites)
    )]
    NeedsRewrite { rewrites: Vec<RefName> },
    /// The confirmation does not name exactly the branches to be rewritten.
    #[error(
        "this would rewrite {}, but the request confirmed {}; confirm exactly the branches the plan names",
        names(.expected),
        names(.confirmed)
    )]
    Unconfirmed {
        expected: Vec<RefName>,
        confirmed: Vec<RefName>,
    },
}

fn names(branches: &[RefName]) -> String {
    if branches.is_empty() {
        return "no branches".into();
    }
    branches
        .iter()
        .map(|b| format!("`{b}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn heads(members: &[PlanMember]) -> Vec<RefName> {
    members.iter().map(|m| m.head.clone()).collect()
}

/// The dry run's answer for these members.
pub fn preview(members: &[PlanMember]) -> StackRewritePlan {
    StackRewritePlan {
        rewrites: members
            .iter()
            .map(|m| RewriteBranch {
                number: m.number,
                branch: m.head.clone(),
            })
            .collect(),
    }
}

fn require_confirmation(
    rewrites: &[PlanMember],
    confirmed: &[RefName],
) -> Result<(), StackRequestError> {
    let expected = heads(rewrites);
    if confirms_exactly(confirmed, &expected) {
        Ok(())
    } else {
        Err(StackRequestError::Unconfirmed {
            expected,
            confirmed: confirmed.to_vec(),
        })
    }
}

/// "Make stack": valid, and nothing to rewrite.
pub fn make(
    state: &RepoState,
    prs: &[PrNumber],
    trunk: RefName,
) -> Result<StackPlan, StackRequestError> {
    let plan = plan_stack(state, prs, trunk)?;
    if plan.needs_rewrite() {
        return Err(StackRequestError::NeedsRewrite {
            rewrites: heads(plan.rewrites()),
        });
    }
    Ok(plan)
}

/// "Arrange": valid, and the confirmation names exactly what will be
/// rewritten.
pub fn arrange(
    state: &RepoState,
    prs: &[PrNumber],
    trunk: RefName,
    confirmed: &[RefName],
) -> Result<StackPlan, StackRequestError> {
    let plan = plan_stack(state, prs, trunk)?;
    require_confirmation(plan.rewrites(), confirmed)?;
    Ok(plan)
}

/// "Add to stack": valid, and the confirmation names exactly what will be
/// rewritten.
pub fn extend(
    state: &RepoState,
    stack: StackNumber,
    prs: &[PrNumber],
    confirmed: &[RefName],
) -> Result<ExtendPlan, StackRequestError> {
    let plan = plan_extend(state, stack, prs)?;
    require_confirmation(plan.rewrites(), confirmed)?;
    Ok(plan)
}

/// Merge and unstack act on an open GitHub stack of the repository.
pub fn existing_stack(state: &RepoState, stack: StackNumber) -> Result<(), StackRequestError> {
    if state.stacks.iter().any(|s| s.number == Some(stack)) {
        Ok(())
    } else {
        Err(StackRequestError::UnknownStack {
            repo: state.id.clone(),
            stack,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{github_stack, pull, repo_state};

    fn n(numbers: &[u32]) -> Vec<PrNumber> {
        numbers.iter().copied().map(PrNumber).collect()
    }

    fn r(name: &str) -> RefName {
        RefName::new(name).expect("valid")
    }

    fn seven() -> StackNumber {
        StackNumber::new(7).expect("non-zero")
    }

    /// #1 a←main, #2 b←main (not chained), #3 c←b.
    fn unchained() -> RepoState {
        repo_state(
            vec![
                pull(1, "a", "main"),
                pull(2, "b", "main"),
                pull(3, "c", "b"),
            ],
            vec![],
        )
    }

    #[test]
    fn make_accepts_a_chain_and_refuses_one_that_needs_a_rewrite() {
        let chained = repo_state(vec![pull(1, "a", "main"), pull(2, "b", "a")], vec![]);
        assert!(make(&chained, &n(&[1, 2]), r("main")).is_ok());

        assert_eq!(
            make(&unchained(), &n(&[1, 2, 3]), r("main")),
            Err(StackRequestError::NeedsRewrite {
                rewrites: vec![r("b"), r("c")]
            })
        );
    }

    #[test]
    fn arrange_requires_exactly_the_rewritten_branches() {
        let state = unchained();
        let order = n(&[1, 2, 3]);
        assert!(arrange(&state, &order, r("main"), &[r("b"), r("c")]).is_ok());
        assert!(arrange(&state, &order, r("main"), &[r("c"), r("b")]).is_ok());
        for wrong in [vec![], vec![r("b")], vec![r("a"), r("b"), r("c")]] {
            let err = arrange(&state, &order, r("main"), &wrong).expect_err("mismatch");
            assert_eq!(
                err,
                StackRequestError::Unconfirmed {
                    expected: vec![r("b"), r("c")],
                    confirmed: wrong.clone(),
                }
            );
            assert!(err.to_string().contains("`b`, `c`"), "{err}");
        }
    }

    #[test]
    fn arrange_without_a_rewrite_confirms_nothing() {
        let chained = repo_state(vec![pull(1, "a", "main"), pull(2, "b", "a")], vec![]);
        assert!(arrange(&chained, &n(&[1, 2]), r("main"), &[]).is_ok());
        assert!(matches!(
            arrange(&chained, &n(&[1, 2]), r("main"), &[r("a")]),
            Err(StackRequestError::Unconfirmed { .. })
        ));
    }

    #[test]
    fn closed_or_unknown_pull_requests_are_refused_by_the_plan() {
        assert_eq!(
            arrange(&unchained(), &n(&[1, 9]), r("main"), &[]),
            Err(StackRequestError::Plan(PlanError::NotOpen(PrNumber(9))))
        );
        assert_eq!(
            make(&unchained(), &n(&[1]), r("main")),
            Err(StackRequestError::Plan(PlanError::TooFew))
        );
    }

    /// Stack 7 is #1 a←main, #2 b←a. #3 c←b chains off the top; #4 d←main
    /// does not.
    fn stacked() -> RepoState {
        repo_state(
            vec![
                pull(1, "a", "main"),
                pull(2, "b", "a"),
                pull(3, "c", "b"),
                pull(4, "d", "main"),
            ],
            vec![github_stack(7, &[1, 2])],
        )
    }

    #[test]
    fn extending_with_a_chained_line_confirms_nothing() {
        let plan = extend(&stacked(), seven(), &n(&[3]), &[]).expect("valid");
        assert!(!plan.needs_rewrite());
    }

    #[test]
    fn extending_with_an_unchained_line_must_confirm_it() {
        let state = stacked();
        assert!(extend(&state, seven(), &n(&[4]), &[r("d")]).is_ok());
        assert!(matches!(
            extend(&state, seven(), &n(&[4]), &[]),
            Err(StackRequestError::Unconfirmed { .. })
        ));
        // The stack's own branches are never part of the confirmation.
        assert!(matches!(
            extend(&state, seven(), &n(&[4]), &[r("b"), r("d")]),
            Err(StackRequestError::Unconfirmed { .. })
        ));
    }

    #[test]
    fn an_unknown_stack_is_refused() {
        let eight = StackNumber::new(8).expect("non-zero");
        assert!(matches!(
            extend(&stacked(), eight, &n(&[3]), &[]),
            Err(StackRequestError::Extend(ExtendError::UnknownStack(_)))
        ));
        assert!(existing_stack(&stacked(), seven()).is_ok());
        assert_eq!(
            existing_stack(&stacked(), eight),
            Err(StackRequestError::UnknownStack {
                repo: stacked().id,
                stack: eight
            })
        );
    }

    #[test]
    fn the_preview_lists_branches_bottom_first() {
        let plan = plan_stack(&unchained(), &n(&[1, 2, 3]), r("main")).expect("valid");
        let preview = preview(plan.rewrites());
        assert_eq!(preview.confirm_rewrite(), vec![r("b"), r("c")]);
        assert_eq!(preview.rewrites[0].number, PrNumber(2));
    }
}
