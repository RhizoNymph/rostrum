//! Which of a repository's pull requests render together, under one header.
//!
//! Two sources, in precedence order: stacks GitHub knows about (cached on
//! [`RepoState::stacks`]) and chains detected from base and head branches. A
//! pull request belongs to at most one group. The feed then treats each group
//! as a single [`FeedUnit`] when it orders a repository's rows, so a stack is
//! never split by sorting.

use std::collections::{BTreeSet, HashMap};

use crate::{
    feed::PrIx,
    model::{MergeStatus, PrNumber, PullRequest},
    state::RepoState,
};

use super::{detect::detect_chains, model::Stack};

/// A stack and where its open members sit in `RepoState::prs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackGroup {
    /// The whole stack, including members that are merged, closed, or simply
    /// not in the fetched page of open pull requests.
    pub stack: Stack,
    /// Members present in `RepoState::prs`, bottom first. Never empty.
    pub open: Vec<PrIx>,
}

impl StackGroup {
    /// Members of the stack that are not open here — merged, closed, or
    /// beyond the fetched page.
    pub fn absent(&self) -> usize {
        self.stack.members.len().saturating_sub(self.open.len())
    }

    /// The merge-state summary across the open members.
    pub fn rollup(&self, prs: &[PullRequest]) -> Option<MergeRollup> {
        MergeRollup::of(
            self.open
                .iter()
                .filter_map(|ix| prs.get(ix.0))
                .map(PullRequest::merge_status),
        )
    }
}

/// The groups of one repository: GitHub's stacks with at least one open
/// member, then detected chains of two or more among what is left.
pub fn stack_groups(repo: &RepoState) -> Vec<StackGroup> {
    let index: HashMap<PrNumber, usize> = repo
        .prs
        .iter()
        .enumerate()
        .map(|(ix, pr)| (pr.number, ix))
        .collect();

    let mut claimed = BTreeSet::new();
    let mut groups = Vec::new();

    for stack in repo.stacks.iter().filter(|s| s.repo == repo.id) {
        // A pull request listed by two stacks (which GitHub does not allow)
        // stays with the first, so it can never render twice.
        let open: Vec<PrIx> = stack
            .members
            .as_slice()
            .iter()
            .filter(|number| !claimed.contains(*number))
            .filter_map(|number| index.get(number).map(|ix| PrIx(*ix)))
            .collect();
        claimed.extend(stack.members.as_slice().iter().copied());
        if !open.is_empty() {
            groups.push(StackGroup {
                stack: stack.clone(),
                open,
            });
        }
    }

    for chain in detect_chains(&repo.id, &repo.prs, &claimed) {
        let open: Vec<PrIx> = chain
            .members
            .as_slice()
            .iter()
            .filter_map(|number| index.get(number).map(|ix| PrIx(*ix)))
            .collect();
        groups.push(StackGroup { stack: chain, open });
    }

    groups
}

/// Every member's merge state folded into what a header can say in a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeRollup {
    /// Members whose state is [`MergeStatus::Ready`] or
    /// [`MergeStatus::Unstable`] — mergeable as far as GitHub has said.
    pub mergeable: usize,
    pub total: usize,
    /// The most serious state among the members.
    pub worst: MergeStatus,
}

impl MergeRollup {
    pub fn of(statuses: impl IntoIterator<Item = MergeStatus>) -> Option<Self> {
        let mut rollup: Option<Self> = None;
        for status in statuses {
            let entry = rollup.get_or_insert(Self {
                mergeable: 0,
                total: 0,
                worst: status,
            });
            entry.total += 1;
            if !status.blocks_merge() {
                entry.mergeable += 1;
            }
            if severity(status) > severity(entry.worst) {
                entry.worst = status;
            }
        }
        rollup
    }

    pub fn all_mergeable(&self) -> bool {
        self.mergeable == self.total
    }

    /// Header text: `ready`, or `2/3 ready · conflict`.
    pub fn label(&self) -> String {
        if self.all_mergeable() {
            return if self.worst == MergeStatus::Unstable {
                "ready · checks failing".into()
            } else {
                "ready".into()
            };
        }
        let word = match self.worst {
            MergeStatus::Conflicts => "conflict",
            MergeStatus::Blocked => "blocked",
            MergeStatus::Behind => "behind",
            MergeStatus::Draft => "draft",
            MergeStatus::Computing => "computing",
            MergeStatus::Unstable | MergeStatus::Ready => "ready",
        };
        format!("{}/{} ready · {word}", self.mergeable, self.total)
    }
}

/// How much a state stands between the stack and a merge. Conflicts first:
/// they are the one thing the author must act on whatever the rules say.
fn severity(status: MergeStatus) -> u8 {
    match status {
        MergeStatus::Ready => 0,
        MergeStatus::Unstable => 1,
        MergeStatus::Computing => 2,
        MergeStatus::Draft => 3,
        MergeStatus::Behind => 4,
        MergeStatus::Blocked => 5,
        MergeStatus::Conflicts => 6,
    }
}

/// Index into the feed's list of stack groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StackIx(pub usize);

/// One thing the feed orders: a lone pull request, or a stack's visible
/// members, bottom first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedUnit {
    Single(PrIx),
    Stack {
        /// Index into the groups passed to [`units`].
        group: usize,
        /// Never empty.
        visible: Vec<PrIx>,
    },
}

impl FeedUnit {
    /// The unit's pull requests, bottom first.
    pub fn members(&self) -> &[PrIx] {
        match self {
            Self::Single(ix) => std::slice::from_ref(ix),
            Self::Stack { visible, .. } => visible,
        }
    }
}

/// Partition one repository's visible pull requests into units.
///
/// The default order is the order of `visible`: a group takes the place of
/// its first visible member, and later members are pulled up next to it.
/// That keeps the feed's existing order (most recently updated first) for
/// lone pull requests and puts a stack where its most recent member was.
pub fn units(visible: &[PrIx], groups: &[StackGroup]) -> Vec<FeedUnit> {
    let mut group_of: HashMap<PrIx, usize> = HashMap::new();
    for (gix, group) in groups.iter().enumerate() {
        for member in &group.open {
            group_of.entry(*member).or_insert(gix);
        }
    }
    let shown: BTreeSet<PrIx> = visible.iter().copied().collect();

    let mut emitted = BTreeSet::new();
    let mut out = Vec::new();
    for ix in visible {
        match group_of.get(ix) {
            None => out.push(FeedUnit::Single(*ix)),
            Some(gix) => {
                if !emitted.insert(*gix) {
                    continue;
                }
                let members: Vec<PrIx> = groups[*gix]
                    .open
                    .iter()
                    .copied()
                    .filter(|member| shown.contains(member))
                    .collect();
                out.push(FeedUnit::Stack {
                    group: *gix,
                    visible: members,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{MergeStateStatus, Mergeable, RepoId},
        stack::model::{RefName, StackMembers, StackNumber},
        state::LoadState,
        test_support::pull,
    };

    fn link(number: u32, head: &str, base: &str) -> PullRequest {
        let mut pr = pull(number);
        pr.head_ref = head.into();
        pr.base_ref = base.into();
        pr
    }

    fn repo(prs: Vec<PullRequest>, stacks: Vec<Stack>) -> RepoState {
        RepoState {
            id: RepoId::new("o", "r"),
            prs,
            load: LoadState::Idle,
            collapsed: false,
            stacks,
        }
    }

    fn github(number: u32, members: &[u32]) -> Stack {
        Stack {
            repo: RepoId::new("o", "r"),
            number: StackNumber::new(number),
            trunk: RefName::new("main").expect("valid"),
            members: StackMembers::new(members.iter().copied().map(PrNumber).collect())
                .expect("valid"),
        }
    }

    fn numbers(repo: &RepoState, group: &StackGroup) -> Vec<u32> {
        group
            .open
            .iter()
            .map(|ix| repo.prs[ix.0].number.0)
            .collect()
    }

    #[test]
    fn github_stacks_come_first_and_keep_their_order() {
        // GitHub's order wins even where the branches would say otherwise.
        let state = repo(
            vec![
                link(5, "x", "main"),
                link(6, "y", "main"),
                link(7, "z", "main"),
            ],
            vec![github(1, &[7, 5, 6])],
        );
        let groups = stack_groups(&state);
        assert_eq!(groups.len(), 1);
        assert_eq!(numbers(&state, &groups[0]), vec![7, 5, 6]);
        assert_eq!(groups[0].absent(), 0);
    }

    #[test]
    fn merged_members_are_counted_as_absent() {
        let state = repo(vec![link(6, "y", "x")], vec![github(1, &[5, 6])]);
        let groups = stack_groups(&state);
        assert_eq!(numbers(&state, &groups[0]), vec![6]);
        assert_eq!(groups[0].absent(), 1);
    }

    #[test]
    fn a_github_stack_with_nothing_open_is_dropped() {
        let state = repo(vec![link(9, "q", "main")], vec![github(1, &[5, 6])]);
        assert!(stack_groups(&state).is_empty());
    }

    #[test]
    fn a_stack_for_another_repository_is_ignored() {
        let mut other = github(1, &[1, 2]);
        other.repo = RepoId::new("someone", "else");
        let state = repo(
            vec![link(1, "a", "main"), link(2, "b", "main")],
            vec![other],
        );
        assert!(stack_groups(&state).is_empty());
    }

    #[test]
    fn detected_chains_follow_and_skip_claimed_members() {
        let state = repo(
            vec![
                link(1, "a", "main"),
                link(2, "b", "a"),
                link(3, "c", "main"),
                link(4, "d", "c"),
            ],
            vec![github(8, &[1, 2])],
        );
        let groups = stack_groups(&state);
        assert_eq!(groups.len(), 2);
        assert!(groups[0].stack.is_on_github());
        assert_eq!(numbers(&state, &groups[0]), vec![1, 2]);
        assert!(!groups[1].stack.is_on_github());
        assert_eq!(numbers(&state, &groups[1]), vec![3, 4]);
    }

    #[test]
    fn a_pull_request_in_two_github_stacks_stays_with_the_first() {
        let state = repo(
            vec![link(1, "a", "main"), link(2, "b", "a"), link(3, "c", "b")],
            vec![github(1, &[1, 2]), github(2, &[2, 3])],
        );
        let groups = stack_groups(&state);
        assert_eq!(numbers(&state, &groups[0]), vec![1, 2]);
        assert_eq!(numbers(&state, &groups[1]), vec![3]);
    }

    fn with_merge(number: u32, mergeable: Mergeable, state: MergeStateStatus) -> PullRequest {
        let mut pr = pull(number);
        pr.mergeable = mergeable;
        pr.merge_state = state;
        pr
    }

    #[test]
    fn the_rollup_reports_the_worst_state_and_the_ready_count() {
        let prs = [
            with_merge(1, Mergeable::Mergeable, MergeStateStatus::Clean),
            with_merge(2, Mergeable::Conflicting, MergeStateStatus::Dirty),
            with_merge(3, Mergeable::Mergeable, MergeStateStatus::Blocked),
        ];
        let rollup = MergeRollup::of(prs.iter().map(PullRequest::merge_status)).expect("some");
        assert_eq!(rollup.total, 3);
        assert_eq!(rollup.mergeable, 1);
        assert_eq!(rollup.worst, MergeStatus::Conflicts);
        assert_eq!(rollup.label(), "1/3 ready · conflict");
    }

    #[test]
    fn an_all_ready_rollup_says_ready_and_flags_red_checks() {
        let clean = with_merge(1, Mergeable::Mergeable, MergeStateStatus::Clean);
        let rollup = MergeRollup::of([clean.merge_status()]).expect("some");
        assert_eq!(rollup.label(), "ready");
        assert!(rollup.all_mergeable());

        let red = with_merge(2, Mergeable::Mergeable, MergeStateStatus::Unstable);
        let rollup = MergeRollup::of([clean.merge_status(), red.merge_status()]).expect("some");
        assert_eq!(rollup.label(), "ready · checks failing");
    }

    #[test]
    fn severity_orders_every_state() {
        let ordered = [
            MergeStatus::Ready,
            MergeStatus::Unstable,
            MergeStatus::Computing,
            MergeStatus::Draft,
            MergeStatus::Behind,
            MergeStatus::Blocked,
            MergeStatus::Conflicts,
        ];
        for pair in ordered.windows(2) {
            assert!(severity(pair[0]) < severity(pair[1]), "{pair:?}");
        }
        assert_eq!(MergeRollup::of([]), None);
    }

    #[test]
    fn units_keep_lone_pull_requests_in_place_and_pull_stacks_together() {
        let state = repo(
            vec![
                link(10, "lone1", "main"),
                link(2, "b", "a"),
                link(11, "lone2", "main"),
                link(1, "a", "main"),
            ],
            vec![],
        );
        let groups = stack_groups(&state);
        let visible: Vec<PrIx> = (0..4).map(PrIx).collect();
        let units = units(&visible, &groups);
        assert_eq!(
            units,
            vec![
                FeedUnit::Single(PrIx(0)),
                // The stack takes #2's place (its first visible member), bottom
                // first.
                FeedUnit::Stack {
                    group: 0,
                    visible: vec![PrIx(3), PrIx(1)],
                },
                FeedUnit::Single(PrIx(2)),
            ]
        );
    }

    #[test]
    fn hidden_members_are_left_out_of_their_unit() {
        let state = repo(
            vec![link(1, "a", "main"), link(2, "b", "a"), link(3, "c", "b")],
            vec![],
        );
        let groups = stack_groups(&state);
        let units = units(&[PrIx(0), PrIx(2)], &groups);
        assert_eq!(
            units,
            vec![FeedUnit::Stack {
                group: 0,
                visible: vec![PrIx(0), PrIx(2)],
            }]
        );
        assert_eq!(units[0].members(), &[PrIx(0), PrIx(2)]);
    }

    #[test]
    fn a_stack_with_no_visible_member_has_no_unit() {
        let state = repo(
            vec![
                link(1, "a", "main"),
                link(2, "b", "a"),
                link(3, "z", "main"),
            ],
            vec![],
        );
        let groups = stack_groups(&state);
        assert_eq!(units(&[PrIx(2)], &groups), vec![FeedUnit::Single(PrIx(2))]);
    }
}
