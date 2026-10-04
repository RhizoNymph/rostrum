//! Requests checked on the phone: parsing what Kotlin sent into the
//! protocol's typed values, and the cheap local answers — is this plan valid
//! against the cached feed, which pull requests could be added to a stack —
//! from `rostrum-core`'s `plan_stack` / `plan_extend`, the rules the desktop
//! enforces. The desktop decides against GitHub as it is now; these only let
//! the phone show eligibility and refuse the obviously wrong before a round
//! trip.

use std::collections::BTreeSet;

use rostrum_core::{PrNumber, RefName, RepoId, RepoState, StackNumber, plan_extend, plan_stack};
use rostrum_remote::StackPlanRequest as WirePlanRequest;

use crate::{
    engine::state::parse_repo,
    error::RostrumError,
    stack_actions::{
        StackCandidate, StackEligibility, StackPlanCheck, StackPlanRequest, StackRewrite,
    },
};

pub(crate) fn stack_number(stack: u32) -> Result<StackNumber, RostrumError> {
    StackNumber::new(stack)
        .ok_or_else(|| RostrumError::invalid("stack 0 is not a stack".to_string()))
}

pub(crate) fn ref_name(name: &str, what: &str) -> Result<RefName, RostrumError> {
    RefName::new(name.trim())
        .map_err(|error| RostrumError::invalid(format!("`{name}` is not a valid {what}: {error}")))
}

/// Pull request numbers, bottom first: at least `min`, each once.
pub(crate) fn members(prs: &[u32], min: usize) -> Result<Vec<PrNumber>, RostrumError> {
    if prs.len() < min {
        return Err(RostrumError::invalid(if min > 1 {
            format!("a stack needs at least {min} pull requests")
        } else {
            "pick at least one pull request".to_string()
        }));
    }
    let mut seen = BTreeSet::new();
    prs.iter()
        .map(|&n| {
            if seen.insert(n) {
                Ok(PrNumber(n))
            } else {
                Err(RostrumError::invalid(format!("#{n} is listed twice")))
            }
        })
        .collect()
}

pub(crate) fn branches(names: &[String]) -> Result<Vec<RefName>, RostrumError> {
    names.iter().map(|name| ref_name(name, "branch")).collect()
}

/// A plan request, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanInput {
    Arrange {
        repo: RepoId,
        prs: Vec<PrNumber>,
        trunk: RefName,
    },
    Extend {
        repo: RepoId,
        stack: StackNumber,
        prs: Vec<PrNumber>,
    },
}

impl PlanInput {
    pub(crate) fn parse(request: &StackPlanRequest) -> Result<Self, RostrumError> {
        Ok(match request {
            StackPlanRequest::Arrange { repo, prs, trunk } => Self::Arrange {
                repo: parse_repo(repo)?,
                prs: members(prs, 2)?,
                trunk: ref_name(trunk, "trunk")?,
            },
            StackPlanRequest::Extend { repo, stack, prs } => Self::Extend {
                repo: parse_repo(repo)?,
                stack: stack_number(*stack)?,
                prs: members(prs, 1)?,
            },
        })
    }

    pub(crate) fn repo(&self) -> &RepoId {
        match self {
            Self::Arrange { repo, .. } | Self::Extend { repo, .. } => repo,
        }
    }

    pub(crate) fn to_wire(&self) -> WirePlanRequest {
        match self.clone() {
            Self::Arrange { repo, prs, trunk } => WirePlanRequest::Arrange { repo, prs, trunk },
            Self::Extend { repo, stack, prs } => WirePlanRequest::Extend { repo, stack, prs },
        }
    }
}

fn rewrites(members: &[rostrum_core::PlanMember]) -> Vec<StackRewrite> {
    members
        .iter()
        .map(|member| StackRewrite {
            number: member.number.0,
            branch: member.head.as_str().to_string(),
        })
        .collect()
}

/// The plan as the cached feed sees it.
pub(crate) fn check(repo: &RepoState, input: &PlanInput) -> StackPlanCheck {
    let outcome = match input {
        PlanInput::Arrange { prs, trunk, .. } => plan_stack(repo, prs, trunk.clone())
            .map(|plan| rewrites(plan.rewrites()))
            .map_err(|error| error.to_string()),
        PlanInput::Extend { stack, prs, .. } => plan_extend(repo, *stack, prs)
            .map(|plan| rewrites(plan.rewrites()))
            .map_err(|error| error.to_string()),
    };
    match outcome {
        Ok(rewrites) => StackPlanCheck::Valid { rewrites },
        Err(reason) => StackPlanCheck::Invalid { reason },
    }
}

/// Every open pull request not already in stack `stack`, with whether it
/// alone could be added to its top. `InvalidInput` when the cached feed does
/// not know the stack.
pub(crate) fn candidates(
    repo: &RepoState,
    stack: StackNumber,
) -> Result<Vec<StackCandidate>, RostrumError> {
    let target = repo
        .stacks
        .iter()
        .find(|known| known.number == Some(stack))
        .ok_or_else(|| {
            RostrumError::invalid(format!(
                "stack {stack} is not one of {}'s open stacks",
                repo.id
            ))
        })?;
    let mut open: Vec<_> = repo
        .prs
        .iter()
        .filter(|pr| !target.members.as_slice().contains(&pr.number))
        .collect();
    open.sort_by_key(|pr| pr.number);
    Ok(open
        .into_iter()
        .map(|pr| StackCandidate {
            number: pr.number.0,
            title: pr.title.clone(),
            eligibility: match plan_extend(repo, stack, &[pr.number]) {
                Ok(plan) => StackEligibility::Eligible {
                    chained: plan.is_chained(0),
                },
                Err(error) => StackEligibility::Ineligible {
                    reason: error.to_string(),
                },
            },
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use rostrum_core::{Stack, StackMembers};

    use super::*;
    use crate::test_support::pull;

    /// `a/b`: stack 7 = #1 → #2 on main; #3 chained off #2's head; #4 on
    /// main; #5 from a fork.
    fn repo() -> RepoState {
        let mut repo = RepoState::new("a/b".parse().expect("repo"));
        let mut prs = vec![pull(1), pull(2), pull(3), pull(4), pull(5)];
        prs[1].base_ref = prs[0].head_ref.clone();
        prs[2].base_ref = prs[1].head_ref.clone();
        prs[4].is_cross_repository = true;
        repo.prs = prs;
        repo.stacks = vec![Stack {
            repo: repo.id.clone(),
            number: StackNumber::new(7),
            trunk: RefName::new("main").expect("ref"),
            members: StackMembers::new(vec![PrNumber(1), PrNumber(2)]).expect("members"),
        }];
        repo
    }

    fn seven() -> StackNumber {
        StackNumber::new(7).expect("non-zero")
    }

    #[test]
    fn requests_parse_into_typed_values_or_say_what_is_wrong() {
        let arrange = StackPlanRequest::Arrange {
            repo: "a/b".into(),
            prs: vec![3, 4],
            trunk: " main ".into(),
        };
        let parsed = PlanInput::parse(&arrange).expect("valid");
        assert_eq!(parsed.repo(), &"a/b".parse::<RepoId>().expect("repo"));
        assert!(matches!(parsed.to_wire(), WirePlanRequest::Arrange { .. }));
        for bad in [
            StackPlanRequest::Arrange {
                repo: "a/b".into(),
                prs: vec![3],
                trunk: "main".into(),
            },
            StackPlanRequest::Arrange {
                repo: "a/b".into(),
                prs: vec![3, 3],
                trunk: "main".into(),
            },
            StackPlanRequest::Arrange {
                repo: "a/b".into(),
                prs: vec![3, 4],
                trunk: "--force".into(),
            },
            StackPlanRequest::Extend {
                repo: "a/b".into(),
                stack: 0,
                prs: vec![3],
            },
            StackPlanRequest::Extend {
                repo: "a/b".into(),
                stack: 7,
                prs: vec![],
            },
            StackPlanRequest::Extend {
                repo: "not a repo".into(),
                stack: 7,
                prs: vec![3],
            },
        ] {
            assert!(
                matches!(
                    PlanInput::parse(&bad),
                    Err(RostrumError::InvalidInput { .. } | RostrumError::InvalidRepo { .. })
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_local_check_names_rewrites_or_the_problem() {
        let repo = repo();
        let chained = PlanInput::Extend {
            repo: repo.id.clone(),
            stack: seven(),
            prs: vec![PrNumber(3)],
        };
        assert_eq!(
            check(&repo, &chained),
            StackPlanCheck::Valid { rewrites: vec![] }
        );
        let rebased = PlanInput::Extend {
            repo: repo.id.clone(),
            stack: seven(),
            prs: vec![PrNumber(4)],
        };
        assert_eq!(
            check(&repo, &rebased),
            StackPlanCheck::Valid {
                rewrites: vec![StackRewrite {
                    number: 4,
                    branch: "branch-4".into()
                }]
            }
        );
        let stacked = PlanInput::Arrange {
            repo: repo.id.clone(),
            prs: vec![PrNumber(2), PrNumber(4)],
            trunk: RefName::new("main").expect("ref"),
        };
        let StackPlanCheck::Invalid { reason } = check(&repo, &stacked) else {
            panic!("#2 is already stacked");
        };
        assert!(reason.contains("stack 7"), "{reason}");
    }

    #[test]
    fn candidates_say_which_pull_requests_can_join_a_stack() {
        let repo = repo();
        let found = candidates(&repo, seven()).expect("known stack");
        assert_eq!(
            found.iter().map(|c| c.number).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(
            found[0].eligibility,
            StackEligibility::Eligible { chained: true }
        );
        assert_eq!(
            found[1].eligibility,
            StackEligibility::Eligible { chained: false }
        );
        assert!(matches!(
            &found[2].eligibility,
            StackEligibility::Ineligible { reason } if reason.contains("fork")
        ));
        assert!(candidates(&repo, StackNumber::new(8).expect("n")).is_err());
    }
}
