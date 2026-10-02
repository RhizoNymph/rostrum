//! The comparisons the branch view asks GitHub for, and their answers.

use std::collections::BTreeMap;

use crate::model::{Divergence, PrNumber, PullRequest};

use super::{name::TrunkName, trunks::Trunks};

/// What one comparison in a batch is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareKey {
    /// A trunk against the default branch.
    Trunk(TrunkName),
    /// A pull request's head against its base.
    Pull(PrNumber),
}

/// Every comparison the branch view needs, as one batch.
///
/// `pairs[i]` is `(base, head)` — the shape `GitHubClient::divergences`
/// takes, so the whole view costs one aliased document — and `keys[i]` says
/// what that pair is about, so answers are filed by identity rather than by
/// position once they come back.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ComparePlan {
    keys: Vec<CompareKey>,
    pairs: Vec<(String, String)>,
}

/// Why a batch's answers could not be filed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("asked {expected} comparisons but got {got} answers")]
    AnswerCount { expected: usize, got: usize },
}

impl ComparePlan {
    /// Each existing trunk against the default branch, then each pull
    /// request against its base.
    ///
    /// A configured trunk GitHub does not have is left out — there is
    /// nothing to compare — and so is the default branch itself, whose
    /// distance from itself is not news. Every pull request is included,
    /// cross-fork ones too: GitHub answers those with a per-alias
    /// `NOT_FOUND` the batch tolerates, which files as "unknown".
    pub fn new(trunks: &Trunks, prs: &[PullRequest]) -> Self {
        let mut plan = Self::default();
        let default = trunks.default_branch();
        for other in trunks.others().iter().filter(|other| other.exists) {
            plan.keys.push(CompareKey::Trunk(other.name.clone()));
            plan.pairs.push((
                default.as_str().to_string(),
                other.name.as_str().to_string(),
            ));
        }
        for pr in prs {
            plan.keys.push(CompareKey::Pull(pr.number));
            plan.pairs.push((pr.base_ref.clone(), pr.head_ref.clone()));
        }
        plan
    }

    pub fn keys(&self) -> &[CompareKey] {
        &self.keys
    }

    /// `(base, head)` per comparison, index-aligned with [`Self::keys`].
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// File a batch's answers under what they were about.
    ///
    /// `answers` must be index-aligned with the plan; anything else means
    /// the answers belong to some other question, and filing them would put
    /// counts on the wrong rows, so it is refused outright. A `None` answer
    /// is simply not filed, which is how "unknown" is represented.
    pub fn answer(&self, answers: Vec<Option<Divergence>>) -> Result<BranchCounts, PlanError> {
        if answers.len() != self.keys.len() {
            return Err(PlanError::AnswerCount {
                expected: self.keys.len(),
                got: answers.len(),
            });
        }
        let mut counts = BranchCounts::default();
        for (key, answer) in self.keys.iter().zip(answers) {
            let Some(divergence) = answer else {
                continue;
            };
            match key {
                CompareKey::Trunk(name) => {
                    counts.trunks.insert(name.clone(), divergence);
                }
                CompareKey::Pull(number) => {
                    counts.pulls.insert(*number, divergence);
                }
            }
        }
        Ok(counts)
    }
}

/// The answered comparisons of one batch, by identity.
///
/// Absence means unknown: cross-fork, a deleted branch, or not asked yet.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BranchCounts {
    trunks: BTreeMap<TrunkName, Divergence>,
    pulls: BTreeMap<PrNumber, Divergence>,
}

impl BranchCounts {
    /// How far `trunk` is from the default branch, if known.
    pub fn trunk(&self, trunk: &TrunkName) -> Option<Divergence> {
        self.trunks.get(trunk).copied()
    }

    /// How far a pull request's head is from its base, if known.
    pub fn pull(&self, number: PrNumber) -> Option<Divergence> {
        self.pulls.get(&number).copied()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{branches::trunks::TrunkChoice, test_support::pull};

    fn name(raw: &str) -> TrunkName {
        TrunkName::parse(raw).expect("valid test name")
    }

    fn trunks(default: &str, configured: &[&str], existing: &[&str]) -> Trunks {
        Trunks::resolve(
            name(default),
            &TrunkChoice::Configured(configured.iter().map(|raw| name(raw)).collect()),
            &existing
                .iter()
                .map(|raw| name(raw))
                .collect::<BTreeSet<_>>(),
        )
    }

    fn pr(number: u32, base: &str, head: &str) -> PullRequest {
        let mut pr = pull(number);
        pr.base_ref = base.into();
        pr.head_ref = head.into();
        pr
    }

    #[test]
    fn trunks_come_first_against_the_default_then_pulls_against_their_base() {
        let plan = ComparePlan::new(
            &trunks("main", &["staging", "develop"], &["staging", "develop"]),
            &[pr(7, "staging", "feat-a"), pr(3, "main", "fix-b")],
        );
        assert_eq!(
            plan.keys(),
            [
                CompareKey::Trunk(name("staging")),
                CompareKey::Trunk(name("develop")),
                CompareKey::Pull(PrNumber(7)),
                CompareKey::Pull(PrNumber(3)),
            ]
        );
        let pairs: Vec<(&str, &str)> = plan
            .pairs()
            .iter()
            .map(|(base, head)| (base.as_str(), head.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("main", "staging"),
                ("main", "develop"),
                ("staging", "feat-a"),
                ("main", "fix-b"),
            ]
        );
    }

    #[test]
    fn a_missing_trunk_is_not_compared() {
        let plan = ComparePlan::new(&trunks("main", &["qa", "staging"], &["staging"]), &[]);
        assert_eq!(plan.keys(), [CompareKey::Trunk(name("staging"))]);
    }

    #[test]
    fn a_repository_with_only_a_default_branch_and_no_prs_asks_nothing() {
        let plan = ComparePlan::new(&trunks("main", &[], &[]), &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.answer(Vec::new()), Ok(BranchCounts::default()));
    }

    #[test]
    fn answers_file_by_identity_and_unknowns_stay_absent() {
        let plan = ComparePlan::new(
            &trunks("main", &["staging"], &["staging"]),
            &[pr(7, "staging", "a"), pr(3, "main", "b")],
        );
        let counts = plan
            .answer(vec![
                Some(Divergence::new(4, 1)),
                None,
                Some(Divergence::new(2, 0)),
            ])
            .expect("aligned answers");
        assert_eq!(counts.trunk(&name("staging")), Some(Divergence::new(4, 1)));
        assert_eq!(counts.pull(PrNumber(7)), None);
        assert_eq!(counts.pull(PrNumber(3)), Some(Divergence::new(2, 0)));
        assert_eq!(counts.trunk(&name("main")), None);
    }

    #[test]
    fn misaligned_answers_are_refused_rather_than_misfiled() {
        let plan = ComparePlan::new(&trunks("main", &[], &[]), &[pr(1, "main", "a")]);
        assert_eq!(
            plan.answer(vec![None, None]),
            Err(PlanError::AnswerCount {
                expected: 1,
                got: 2
            })
        );
    }
}
