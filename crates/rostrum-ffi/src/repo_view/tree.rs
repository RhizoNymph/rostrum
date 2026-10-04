//! The core's branch tree as rows Kotlin draws. Placement, trunk resolution
//! and counting are `rostrum_core::branches`', shared with the desktop.

use std::collections::HashMap;

use rostrum_core::{
    Divergence, LoginKey, PrNumber, RepoState,
    branches::{BranchRow as CoreRow, BranchTree as CoreTree, PullNote, TrunkDrift as CoreDrift},
    stack::stack_groups,
};

use crate::{
    feed::{count, summary::summarize},
    repo_view::{BranchDrift, BranchNote, BranchRow, TrunkDrift},
};

impl From<Divergence> for BranchDrift {
    fn from(divergence: Divergence) -> Self {
        Self {
            ahead: divergence.ahead,
            behind: divergence.behind,
        }
    }
}

fn drift(drift: CoreDrift) -> TrunkDrift {
    match drift {
        CoreDrift::Default => TrunkDrift::Default,
        CoreDrift::Missing => TrunkDrift::Missing,
        CoreDrift::Unknown => TrunkDrift::Unknown,
        CoreDrift::Known(divergence) => TrunkDrift::Known {
            drift: divergence.into(),
        },
    }
}

/// `stack 7` or `chain` for every pull request in a group, as the feed groups.
fn stack_labels(repo: &RepoState) -> HashMap<PrNumber, String> {
    let mut labels = HashMap::new();
    for group in stack_groups(repo) {
        let label = match group.stack.number {
            Some(number) => format!("stack {number}"),
            None => "chain".to_string(),
        };
        for ix in &group.open {
            if let Some(pr) = repo.prs.get(ix.0) {
                labels.insert(pr.number, label.clone());
            }
        }
    }
    labels
}

/// The tree's rows, each pull request carrying its feed row when loaded.
pub(crate) fn rows(tree: &CoreTree, repo: &RepoState, viewer: Option<&LoginKey>) -> Vec<BranchRow> {
    let labels = stack_labels(repo);
    tree.rows()
        .into_iter()
        .map(|row| match row {
            CoreRow::Trunk {
                name,
                drift: d,
                pulls,
            } => BranchRow::Trunk {
                name: name.as_str().to_string(),
                drift: drift(d),
                pulls: count(pulls),
            },
            CoreRow::OtherBases => BranchRow::OtherBases,
            CoreRow::Base { name, pulls } => BranchRow::Base {
                name,
                pulls: count(pulls),
            },
            CoreRow::Pull {
                depth,
                number,
                head,
                base,
                drift: d,
                note,
            } => BranchRow::Pull {
                depth: count(depth),
                number: number.0,
                head,
                base,
                drift: d.map(Into::into),
                note: note.map(|note| match note {
                    PullNote::BreaksCycle => BranchNote::BreaksCycle,
                    PullNote::AmbiguousBase => BranchNote::AmbiguousBase,
                }),
                stack_label: labels.get(&number).cloned(),
                pull: repo
                    .prs
                    .iter()
                    .find(|pr| pr.number == number)
                    .map(|pr| summarize(&repo.id, pr, viewer)),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rostrum_core::branches::{
        BranchCounts, ComparePlan, TrunkChoice, TrunkName, Trunks, build_tree,
    };

    use super::*;
    use crate::test_support::pull;

    fn trunk(name: &str) -> TrunkName {
        TrunkName::parse(name).expect("trunk")
    }

    /// `main` (default) and `staging`; #1 on main, #2 on #1's head (a chain),
    /// #3 on staging, #4 on an unknown base.
    fn repo() -> RepoState {
        let mut repo = RepoState::new("a/b".parse().expect("repo"));
        let mut prs = vec![pull(1), pull(2), pull(3), pull(4)];
        prs[1].base_ref = prs[0].head_ref.clone();
        prs[2].base_ref = "staging".into();
        prs[3].base_ref = "somewhere".into();
        repo.prs = prs;
        repo
    }

    fn tree(repo: &RepoState) -> (CoreTree, BranchCounts) {
        let existing: BTreeSet<TrunkName> = [trunk("main"), trunk("staging")].into();
        let trunks = Trunks::resolve(trunk("main"), &TrunkChoice::Detected, &existing);
        let plan = ComparePlan::new(&trunks, &repo.prs);
        let answers = plan
            .keys()
            .iter()
            .map(|_| Some(Divergence::new(2, 1)))
            .collect();
        let counts = plan.answer(answers).expect("aligned");
        (build_tree(&trunks, &repo.prs, &counts), counts)
    }

    #[test]
    fn rows_carry_drift_nesting_stacks_and_the_feed_row() {
        let repo = repo();
        let (tree, _) = tree(&repo);
        let rows = rows(&tree, &repo, None);
        let shape: Vec<String> = rows
            .iter()
            .map(|row| match row {
                BranchRow::Trunk { name, drift, pulls } => format!("T {name} {drift:?} {pulls}"),
                BranchRow::OtherBases => "Other".into(),
                BranchRow::Base { name, pulls } => format!("B {name} {pulls}"),
                BranchRow::Pull {
                    depth,
                    number,
                    stack_label,
                    ..
                } => format!(
                    "P{depth} #{number} {}",
                    stack_label.as_deref().unwrap_or("-")
                ),
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                "T main Default 2",
                "P1 #1 chain",
                "P2 #2 chain",
                "T staging Known { drift: BranchDrift { ahead: 2, behind: 1 } } 1",
                "P1 #3 -",
                "Other",
                "B somewhere 1",
                "P1 #4 -",
            ]
        );
        let BranchRow::Pull { drift, pull, .. } = &rows[1] else {
            panic!("a pull row");
        };
        assert_eq!(
            *drift,
            Some(BranchDrift {
                ahead: 2,
                behind: 1
            })
        );
        assert_eq!(pull.as_ref().map(|pull| pull.number), Some(1));
    }
}
