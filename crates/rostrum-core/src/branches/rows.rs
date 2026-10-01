//! The branch tree flattened into the rows the view draws, top to bottom.

use crate::model::{Divergence, PrNumber};

use super::{
    name::TrunkName,
    tree::{BranchTree, PullNode, PullNote, TrunkDrift},
};

/// One drawn row of the branch tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchRow {
    /// A trunk heading, with how many pull requests sit anywhere beneath it.
    Trunk {
        name: TrunkName,
        drift: TrunkDrift,
        pulls: usize,
    },
    /// The heading over every [`BranchRow::Base`] group. Present only when
    /// there is at least one.
    OtherBases,
    /// A base that is neither a trunk nor one pull request's head.
    Base { name: String, pulls: usize },
    /// A pull request. `depth` is 1 directly under a trunk or base heading
    /// and grows by one per level of stacking.
    Pull {
        depth: usize,
        number: PrNumber,
        head: String,
        base: String,
        drift: Option<Divergence>,
        note: Option<PullNote>,
    },
}

impl BranchTree {
    /// Depth-first, in tree order: each trunk then its stack, then "Other
    /// bases" and each group beneath it.
    pub fn rows(&self) -> Vec<BranchRow> {
        let mut rows = Vec::new();
        for trunk in &self.trunks {
            rows.push(BranchRow::Trunk {
                name: trunk.name.clone(),
                drift: trunk.drift,
                pulls: count(&trunk.pulls),
            });
            push_pulls(&trunk.pulls, 1, &mut rows);
        }
        if !self.other_bases.is_empty() {
            rows.push(BranchRow::OtherBases);
            for group in &self.other_bases {
                rows.push(BranchRow::Base {
                    name: group.base.clone(),
                    pulls: count(&group.pulls),
                });
                push_pulls(&group.pulls, 1, &mut rows);
            }
        }
        rows
    }
}

fn count(nodes: &[PullNode]) -> usize {
    nodes.iter().map(|node| 1 + count(&node.children)).sum()
}

fn push_pulls(nodes: &[PullNode], depth: usize, rows: &mut Vec<BranchRow>) {
    for node in nodes {
        rows.push(BranchRow::Pull {
            depth,
            number: node.number,
            head: node.head.clone(),
            base: node.base.clone(),
            drift: node.drift,
            note: node.note,
        });
        push_pulls(&node.children, depth + 1, rows);
    }
}
