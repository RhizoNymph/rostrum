//! The branch tree: trunks, the pull requests based on them, and the pull
//! requests based on those.
//!
//! Pure: built from the trunks, the open pull requests and whatever counts
//! have arrived, so every placement rule is testable without a network.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::model::{Divergence, PrNumber, PullRequest};

use super::{name::TrunkName, plan::BranchCounts, trunks::Trunks};

/// How far a trunk is from the default branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrunkDrift {
    /// This trunk *is* the default branch, the point everything is measured
    /// from.
    Default,
    /// Configured, but GitHub has no branch by that name.
    Missing,
    /// Exists, but no count has arrived (not fetched yet, or the comparison
    /// failed).
    Unknown,
    Known(Divergence),
}

/// Why a pull request sits where it does, when that is not self-evident.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullNote {
    /// Its base is the head of a pull request that is (transitively) based
    /// on it. The cycle has to be cut somewhere to be drawn at all; it is
    /// cut at the lowest-numbered pull request in it, which is listed under
    /// its base in "Other bases" with this note.
    BreaksCycle,
    /// Its base is the head branch of more than one open pull request —
    /// forks reusing a name — so which one it stacks on cannot be told.
    AmbiguousBase,
}

/// One open pull request in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullNode {
    pub number: PrNumber,
    pub head: String,
    pub base: String,
    /// Head against base: from the branch view's own batch when it has
    /// answered, else whatever the feed's batch last found. `None` when
    /// neither knows — a cross-fork head, or nothing fetched yet.
    pub drift: Option<Divergence>,
    pub note: Option<PullNote>,
    /// Pull requests whose base is this one's head, by number.
    pub children: Vec<PullNode>,
}

/// A trunk and the pull requests based directly on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrunkNode {
    pub name: TrunkName,
    pub drift: TrunkDrift,
    pub pulls: Vec<PullNode>,
}

/// Pull requests whose base is neither a trunk nor exactly one open pull
/// request's head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseGroup {
    pub base: String,
    pub pulls: Vec<PullNode>,
}

/// The whole tree for one repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchTree {
    /// In trunk order: the default branch first.
    pub trunks: Vec<TrunkNode>,
    /// Sorted by base name.
    pub other_bases: Vec<BaseGroup>,
}

/// Where a pull request hangs, before the tree is assembled.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Parent {
    Trunk,
    Pull(PrNumber),
    Other(Option<PullNote>),
}

/// Build the tree.
///
/// Placement, in order of precedence:
///
/// 1. A base that is a trunk puts the pull request under that trunk — even
///    when some pull request's head happens to share the trunk's name, which
///    is what a fork's `main` looks like.
/// 2. A base that is the head of exactly one *other* open pull request nests
///    it under that one, which is how stacks fall out without being declared.
/// 3. Anything else goes to "Other bases", grouped by base name: a base that
///    is no known branch, a base two pull requests' heads share
///    ([`PullNote::AmbiguousBase`]), and a pull request based on its own
///    head name.
///
/// Pull requests whose chain of bases loops never reach a trunk. Each loop is
/// cut at its lowest-numbered member, which goes to "Other bases" with
/// [`PullNote::BreaksCycle`]; the rest of the loop, and anything stacked on
/// it, nests beneath it as usual. Every pull request therefore appears
/// exactly once.
///
/// Siblings are ordered by number. Counts come from `counts`, falling back
/// to each pull request's own `base_divergence`.
pub fn build_tree(trunks: &Trunks, prs: &[PullRequest], counts: &BranchCounts) -> BranchTree {
    let by_number: BTreeMap<PrNumber, &PullRequest> =
        prs.iter().map(|pr| (pr.number, pr)).collect();

    let mut heads: HashMap<&str, Vec<PrNumber>> = HashMap::new();
    for pr in by_number.values() {
        heads
            .entry(pr.head_ref.as_str())
            .or_default()
            .push(pr.number);
    }

    let parents: BTreeMap<PrNumber, Parent> = by_number
        .values()
        .map(|pr| (pr.number, parent_of(pr, trunks, &heads)))
        .collect();

    let mut children: BTreeMap<PrNumber, Vec<PrNumber>> = BTreeMap::new();
    for (number, parent) in &parents {
        if let Parent::Pull(parent) = parent {
            children.entry(*parent).or_default().push(*number);
        }
    }

    let builder = Builder {
        by_number: &by_number,
        children: &children,
        counts,
    };
    let mut placed = BTreeSet::new();

    let trunk_nodes: Vec<TrunkNode> = trunks
        .names()
        .map(|name| {
            let roots: Vec<PrNumber> = by_number
                .values()
                .filter(|pr| pr.base_ref == name.as_str())
                .filter(|pr| parents.get(&pr.number) == Some(&Parent::Trunk))
                .map(|pr| pr.number)
                .collect();
            TrunkNode {
                name: name.clone(),
                drift: trunk_drift(trunks, name, counts),
                pulls: roots
                    .into_iter()
                    .filter_map(|number| builder.node(number, None, &mut placed))
                    .collect(),
            }
        })
        .collect();

    let mut groups: BTreeMap<String, Vec<PullNode>> = BTreeMap::new();
    for (number, parent) in &parents {
        if let Parent::Other(note) = parent
            && let Some(node) = builder.node(*number, *note, &mut placed)
        {
            groups.entry(node.base.clone()).or_default().push(node);
        }
    }

    // Whatever is still unplaced hangs off a loop.
    while let Some(start) = by_number.keys().find(|number| !placed.contains(*number)) {
        let breaker = cycle_breaker(*start, &parents);
        if let Some(node) = builder.node(breaker, Some(PullNote::BreaksCycle), &mut placed) {
            groups.entry(node.base.clone()).or_default().push(node);
        }
    }

    BranchTree {
        trunks: trunk_nodes,
        other_bases: groups
            .into_iter()
            .map(|(base, mut pulls)| {
                pulls.sort_by_key(|node| node.number);
                BaseGroup { base, pulls }
            })
            .collect(),
    }
}

fn parent_of(pr: &PullRequest, trunks: &Trunks, heads: &HashMap<&str, Vec<PrNumber>>) -> Parent {
    if trunks.contains(&pr.base_ref) {
        return Parent::Trunk;
    }
    let owners: Vec<PrNumber> = heads
        .get(pr.base_ref.as_str())
        .map(|owners| {
            owners
                .iter()
                .copied()
                .filter(|owner| *owner != pr.number)
                .collect()
        })
        .unwrap_or_default();
    match owners.as_slice() {
        [] => Parent::Other(None),
        [only] => Parent::Pull(*only),
        _ => Parent::Other(Some(PullNote::AmbiguousBase)),
    }
}

fn trunk_drift(trunks: &Trunks, name: &TrunkName, counts: &BranchCounts) -> TrunkDrift {
    if name == trunks.default_branch() {
        return TrunkDrift::Default;
    }
    let exists = trunks
        .others()
        .iter()
        .find(|other| &other.name == name)
        .is_some_and(|other| other.exists);
    if !exists {
        return TrunkDrift::Missing;
    }
    counts
        .trunk(name)
        .map_or(TrunkDrift::Unknown, TrunkDrift::Known)
}

/// Follow parents from `start` until a pull request repeats; the lowest
/// number on the loop found is where it is cut.
///
/// Only called for pull requests no root reached, whose parent chain cannot
/// end at a trunk or in "Other bases" — those would have been reached — so
/// it must loop.
fn cycle_breaker(start: PrNumber, parents: &BTreeMap<PrNumber, Parent>) -> PrNumber {
    let mut path: Vec<PrNumber> = Vec::new();
    let mut current = start;
    loop {
        if let Some(at) = path.iter().position(|seen| *seen == current) {
            return path[at..].iter().copied().min().unwrap_or(current);
        }
        path.push(current);
        match parents.get(&current) {
            Some(Parent::Pull(next)) => current = *next,
            // Unreachable for an unplaced node; cutting here still places it.
            _ => return current,
        }
    }
}

struct Builder<'a> {
    by_number: &'a BTreeMap<PrNumber, &'a PullRequest>,
    children: &'a BTreeMap<PrNumber, Vec<PrNumber>>,
    counts: &'a BranchCounts,
}

impl Builder<'_> {
    /// The subtree rooted at `number`, or `None` if it is already placed.
    /// Marking before descending is what stops a loop from recursing
    /// forever.
    fn node(
        &self,
        number: PrNumber,
        note: Option<PullNote>,
        placed: &mut BTreeSet<PrNumber>,
    ) -> Option<PullNode> {
        let pr = self.by_number.get(&number)?;
        if !placed.insert(number) {
            return None;
        }
        let children = self
            .children
            .get(&number)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|child| self.node(*child, None, placed))
            .collect();
        Some(PullNode {
            number,
            head: pr.head_ref.clone(),
            base: pr.base_ref.clone(),
            drift: self.counts.pull(number).or(pr.base_divergence),
            note,
            children,
        })
    }
}

impl BranchTree {
    /// How many pull requests the tree holds.
    pub fn pull_count(&self) -> usize {
        fn count(nodes: &[PullNode]) -> usize {
            nodes.iter().map(|node| 1 + count(&node.children)).sum()
        }
        self.trunks
            .iter()
            .map(|trunk| count(&trunk.pulls))
            .chain(self.other_bases.iter().map(|group| count(&group.pulls)))
            .sum()
    }
}
