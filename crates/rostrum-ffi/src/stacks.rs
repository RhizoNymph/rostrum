//! Stacks of pull requests, read-only: a stack's header and its members kept
//! together, in the feed and on a repository's screen.
//!
//! Grouping and order are `rostrum_core`'s — `flatten_tab` and
//! `repo_pull_rows` emit a `StackHeader` row followed by the stack's visible
//! members, bottom first, a stack sorting as one unit — so the phone and the
//! desktop agree on what is a stack and where it sits. This only folds those
//! rows into [`PullItem`]s, so a stack's members cannot be separated from its
//! header on the Kotlin side.
//!
//! Stack actions (make, extend, merge, unstack) run through the paired
//! desktop and are a later phase; nothing here acts.

use rostrum_core::{FeedRow, FeedStack, LoginKey, MergeRollup, RepoState};

use crate::{
    feed::{PrSummary, chips::merge_role, summary::summarize},
    types::{ColorRole, MergeStatus},
};

/// Whether GitHub knows the stack, or rostrum found the chain itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StackKind {
    /// A stack on GitHub (the `gh stack` Stacks API), by its number.
    GitHub { number: u32 },
    /// Open pull requests whose bases chain, which GitHub does not know as a
    /// stack.
    Chain,
}

/// The merge state across a stack's open members.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackRollup {
    /// Members GitHub says can merge (ready or unstable).
    pub mergeable: u32,
    pub total: u32,
    /// The most serious state among them.
    pub worst: MergeStatus,
    /// `ready`, or `2/3 ready · conflict`.
    pub label: String,
    pub role: ColorRole,
}

/// A stack's header.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackSummary {
    pub kind: StackKind,
    /// `Stack 7 · 3 PRs` or `Stackable chain · 3 PRs`.
    pub title: String,
    /// The branch the bottom member targets.
    pub trunk: String,
    /// Every member, open or not.
    pub member_count: u32,
    /// Members merged, closed, or beyond the fetched page.
    pub absent: u32,
    pub rollup: Option<StackRollup>,
}

/// One entry of a repository's pull request list.
// UniFFI cannot carry a `Box` across, and the records are built once per
// snapshot and handed straight to Kotlin, so the size difference costs
// nothing worth an indirection.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum PullItem {
    Single {
        pull: PrSummary,
    },
    /// A stack: its header, then its visible members, bottom first.
    Stack {
        stack: StackSummary,
        members: Vec<PrSummary>,
    },
}

pub(crate) fn stack_summary(placed: &FeedStack, repo: &RepoState) -> StackSummary {
    let group = &placed.group;
    let count = group.stack.members.len();
    let kind = match group.stack.number {
        Some(number) => StackKind::GitHub {
            number: number.get(),
        },
        None => StackKind::Chain,
    };
    let title = match kind {
        StackKind::GitHub { number } => format!("Stack {number} · {count} PRs"),
        StackKind::Chain => format!("Stackable chain · {count} PRs"),
    };
    StackSummary {
        kind,
        title,
        trunk: group.stack.trunk.as_str().to_string(),
        member_count: crate::feed::count(count),
        absent: crate::feed::count(group.absent()),
        rollup: group.rollup(&repo.prs).map(rollup),
    }
}

fn rollup(rollup: MergeRollup) -> StackRollup {
    StackRollup {
        mergeable: crate::feed::count(rollup.mergeable),
        total: crate::feed::count(rollup.total),
        worst: rollup.worst.into(),
        label: rollup.label(),
        role: merge_role(rollup.worst),
    }
}

/// Folds one repository's pull request rows into [`PullItem`]s.
pub(crate) struct PullItems<'a> {
    repo: &'a RepoState,
    stacks: &'a [FeedStack],
    viewer: Option<&'a LoginKey>,
    items: Vec<PullItem>,
    open: Option<(StackSummary, Vec<PrSummary>)>,
}

impl<'a> PullItems<'a> {
    pub(crate) fn new(
        repo: &'a RepoState,
        stacks: &'a [FeedStack],
        viewer: Option<&'a LoginKey>,
    ) -> Self {
        Self {
            repo,
            stacks,
            viewer,
            items: Vec::new(),
            open: None,
        }
    }

    /// Take one row. Rows other than stack headers and pull requests are not
    /// this list's business and are ignored.
    pub(crate) fn push(&mut self, row: FeedRow) {
        match row {
            FeedRow::StackHeader { stack, .. } => {
                self.close();
                if let Some(placed) = self.stacks.get(stack.0) {
                    self.open = Some((stack_summary(placed, self.repo), Vec::new()));
                }
            }
            FeedRow::PrRow { pr, stack, .. } => {
                let Some(pull) = self.repo.prs.get(pr.0) else {
                    return;
                };
                let summary = summarize(&self.repo.id, pull, self.viewer);
                match (stack, self.open.as_mut()) {
                    (Some(_), Some((_, members))) => members.push(summary),
                    _ => {
                        self.close();
                        self.items.push(PullItem::Single { pull: summary });
                    }
                }
            }
            _ => {}
        }
    }

    fn close(&mut self) {
        if let Some((stack, members)) = self.open.take() {
            self.items.push(PullItem::Stack { stack, members });
        }
    }

    pub(crate) fn finish(mut self) -> Vec<PullItem> {
        self.close();
        self.items
    }
}

/// Every pull request in a list of items, in display order.
#[cfg(test)]
pub(crate) fn numbers(items: &[PullItem]) -> Vec<u32> {
    items
        .iter()
        .flat_map(|item| match item {
            PullItem::Single { pull } => vec![pull.number],
            PullItem::Stack { members, .. } => members.iter().map(|pull| pull.number).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rostrum_core::{
        FeedFilter, ItemSortKey, MergeStateStatus, Mergeable, RefName, RepoId, Sort, Stack,
        StackMembers, StackNumber, flatten, repo_pull_rows,
    };

    use super::*;
    use crate::test_support::pull;

    /// `a/b` with #1 (on main), and a chain #2 → #3 → #4 (each based on the
    /// one below's head), plus #5 on main.
    fn repo() -> RepoState {
        let mut repo = RepoState::new("a/b".parse::<RepoId>().expect("repo"));
        let mut prs = vec![pull(1), pull(2), pull(3), pull(4), pull(5)];
        prs[2].base_ref = prs[1].head_ref.clone();
        prs[3].base_ref = prs[2].head_ref.clone();
        prs[1].mergeable = Mergeable::Conflicting;
        prs[1].merge_state = MergeStateStatus::Dirty;
        repo.prs = prs;
        repo.load = rostrum_core::LoadState::Loaded {
            at: chrono::Utc::now(),
        };
        repo
    }

    fn items_of(repo: &RepoState, filter: &FeedFilter) -> Vec<PullItem> {
        let feed = flatten(std::slice::from_ref(repo), filter);
        let mut items = PullItems::new(repo, feed.stacks(), None);
        for row in feed.rows() {
            items.push(*row);
        }
        items.finish()
    }

    #[test]
    fn a_detected_chain_is_one_item_with_its_members_bottom_first() {
        let repo = repo();
        let items = items_of(&repo, &FeedFilter::default());
        let stack = items
            .iter()
            .find_map(|item| match item {
                PullItem::Stack { stack, members } => Some((stack, members)),
                PullItem::Single { .. } => None,
            })
            .expect("a stack item");
        assert_eq!(stack.0.kind, StackKind::Chain);
        assert_eq!(stack.0.title, "Stackable chain · 3 PRs");
        assert_eq!(stack.0.trunk, "main");
        assert_eq!(
            stack.1.iter().map(|pull| pull.number).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        let rollup = stack.0.rollup.as_ref().expect("rollup");
        assert_eq!(rollup.worst, MergeStatus::Conflicts);
        assert_eq!(rollup.role, ColorRole::Danger);
        assert_eq!(rollup.label, "2/3 ready · conflict");
        // Every pull request appears exactly once.
        let mut all = numbers(&items);
        all.sort_unstable();
        assert_eq!(all, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_github_stack_carries_its_number_and_absent_members() {
        let mut repo = repo();
        repo.stacks = vec![Stack {
            repo: repo.id.clone(),
            number: StackNumber::new(7),
            trunk: RefName::new("main").expect("ref"),
            members: StackMembers::new(vec![
                rostrum_core::PrNumber(2),
                rostrum_core::PrNumber(3),
                rostrum_core::PrNumber(4),
                rostrum_core::PrNumber(40),
            ])
            .expect("members"),
        }];
        let items = items_of(&repo, &FeedFilter::default());
        let stack = items
            .iter()
            .find_map(|item| match item {
                PullItem::Stack { stack, .. } => Some(stack),
                PullItem::Single { .. } => None,
            })
            .expect("a stack");
        assert_eq!(stack.kind, StackKind::GitHub { number: 7 });
        assert_eq!(stack.title, "Stack 7 · 4 PRs");
        assert_eq!(stack.member_count, 4);
        assert_eq!(stack.absent, 1);
    }

    #[test]
    fn a_filtered_stack_keeps_only_its_visible_members() {
        let repo = repo();
        let filter = FeedFilter {
            query: "PR 3".into(),
            ..FeedFilter::default()
        };
        let items = items_of(&repo, &filter);
        assert_eq!(numbers(&items), vec![3]);
        assert!(matches!(items[0], PullItem::Stack { .. }));
    }

    #[test]
    fn a_repository_screen_groups_the_same_way_unfiltered() {
        let repo = repo();
        let (rows, stacks) = repo_pull_rows(&repo, Sort::new(ItemSortKey::Title));
        let mut items = PullItems::new(&repo, &stacks, None);
        for row in rows {
            items.push(row);
        }
        let items = items.finish();
        // Title A→Z: "PR 1" < "PR 2…" (the stack, by its bottom member) < "PR 5".
        assert_eq!(numbers(&items), vec![1, 2, 3, 4, 5]);
        assert_eq!(items.len(), 3);
    }
}
