//! Reasons an operation should not be offered yet.
//!
//! A front-end needs to know *before* it draws a button whether pressing it
//! could work, and to say why not when it could not. That is a different
//! question from "did it fail", so it has a different answer: [`Preflight`]
//! returns a list of [`Blocker`]s, not an error. A blocker only becomes
//! [`GitError::Refused`] if the caller asked for the operation anyway.

use crate::{
    refs::{BranchName, Oid},
    status::{InProgress, RepoStatus},
};

/// What the caller is about to do. Carried by errors so a message can name the
/// operation, and used to select which blockers apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Fetch,
    PullRebase,
    Merge,
    Rebase,
    Abort,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::PullRebase => "pull --rebase",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::Abort => "abort",
        }
    }

    /// Whether the operation moves HEAD or touches the worktree. A fetch writes
    /// only `refs/remotes/`, so nothing about the worktree can block it.
    fn touches_worktree(self) -> bool {
        matches!(self, Self::PullRebase | Self::Merge | Self::Rebase)
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether to let git stash and restore local changes around the operation.
///
/// Always passed explicitly to git as `--autostash` or `--no-autostash`. Omitting
/// the flag would let a user's `rebase.autoStash = true` autostash their work
/// with rostrum's checkbox unticked, which is precisely the surprise a checkbox
/// is supposed to prevent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autostash {
    Enabled,
    Disabled,
}

impl Autostash {
    pub fn as_flag(self) -> &'static str {
        match self {
            Self::Enabled => "--autostash",
            Self::Disabled => "--no-autostash",
        }
    }

    pub fn is_enabled(self) -> bool {
        self == Self::Enabled
    }
}

/// One reason an operation would not succeed, with enough detail to explain it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Blocker {
    /// Tracked changes that git would refuse to overwrite. Not raised when
    /// [`Autostash::Enabled`], because that is exactly what autostash handles.
    DirtyWorktree { staged: u32, unstaged: u32 },
    /// Unmerged paths with no sequencer state — what a failed autostash pop
    /// leaves behind. Autostash cannot help here: there is nothing to stash a
    /// conflict into.
    UnresolvedConflicts { count: u32 },
    /// Some other multi-step operation owns the worktree. Never overridden by
    /// autostash: stashing on top of a half-finished rebase would bury it.
    InProgress(InProgress),
    /// HEAD is not on a branch, so there is nothing for a rebase to update.
    DetachedHead { oid: Oid },
    /// The branch has no commits, so it cannot be an argument to `rev-list` and
    /// has nothing to rebase.
    UnbornBranch { name: BranchName },
    /// HEAD is on a different branch than the caller named. Every write command
    /// here uses git's one-argument form specifically so it will not check
    /// anything out, which makes "you are not on that branch" a refusal rather
    /// than something to silently fix.
    WrongBranch {
        expected: BranchName,
        actual: BranchName,
    },
}

impl Blocker {
    /// A sentence to show next to a disabled control.
    pub fn reason(&self) -> String {
        match self {
            Self::DirtyWorktree { staged, unstaged } => format!(
                "the worktree has uncommitted changes ({staged} staged, {unstaged} unstaged)"
            ),
            Self::UnresolvedConflicts { count } => {
                format!("{count} path(s) still have unresolved conflicts")
            }
            Self::InProgress(state) => state.describe().to_string(),
            Self::DetachedHead { oid } => {
                format!("HEAD is detached at {}", oid.short())
            }
            Self::UnbornBranch { name } => format!("`{name}` has no commits yet"),
            Self::WrongBranch { expected, actual } => {
                format!("HEAD is on `{actual}`, not `{expected}`")
            }
        }
    }
}

/// The verdict for one operation against one observed status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preflight {
    operation: Operation,
    blockers: Vec<Blocker>,
}

impl Preflight {
    pub fn new(operation: Operation, blockers: Vec<Blocker>) -> Self {
        Self {
            operation,
            blockers,
        }
    }

    pub fn operation(&self) -> Operation {
        self.operation
    }

    pub fn blockers(&self) -> &[Blocker] {
        &self.blockers
    }

    /// Whether the operation can be attempted.
    pub fn is_clear(&self) -> bool {
        self.blockers.is_empty()
    }

    /// The blocker to show. The first is the most specific, because
    /// [`blockers`] emits them from most to least fundamental.
    pub fn first(&self) -> Option<&Blocker> {
        self.blockers.first()
    }

    /// A ready-made tooltip for a greyed-out control.
    pub fn reason(&self) -> Option<String> {
        self.first().map(Blocker::reason)
    }
}

/// Everything standing between this status and this operation.
///
/// `expected` is the branch the caller believes is checked out — `None` for
/// operations that act on whatever HEAD is on.
///
/// Ordered most fundamental first: an in-progress operation explains a detached
/// HEAD, which explains everything else, so a caller showing one reason shows
/// the useful one.
pub fn blockers(
    operation: Operation,
    status: &RepoStatus,
    expected: Option<&BranchName>,
    autostash: Autostash,
) -> Vec<Blocker> {
    // A fetch writes only `refs/remotes/`; nothing about the worktree can stop
    // it. An abort is only reachable through an `AbortTarget` handed out by
    // `Conflict::abort_target`, which already proves the matching state exists.
    if !operation.touches_worktree() {
        return Vec::new();
    }

    let mut blockers = Vec::new();

    if let Some(state) = status.in_progress {
        blockers.push(Blocker::InProgress(state));
    }
    if status.worktree.conflicted > 0 {
        blockers.push(Blocker::UnresolvedConflicts {
            count: status.worktree.conflicted,
        });
    }

    match &status.head {
        crate::status::Head::Detached { oid } => {
            blockers.push(Blocker::DetachedHead { oid: oid.clone() });
        }
        crate::status::Head::Unborn { name } => {
            blockers.push(Blocker::UnbornBranch { name: name.clone() });
        }
        crate::status::Head::Branch { name, .. } => {
            if let Some(expected) = expected
                && expected != name
            {
                blockers.push(Blocker::WrongBranch {
                    expected: expected.clone(),
                    actual: name.clone(),
                });
            }
        }
    }

    // Checked last because it is the one blocker the caller can clear from the
    // UI, by ticking the autostash box.
    if autostash == Autostash::Disabled
        && (status.worktree.staged > 0 || status.worktree.unstaged > 0)
    {
        blockers.push(Blocker::DirtyWorktree {
            staged: status.worktree.staged,
            unstaged: status.worktree.unstaged,
        });
    }

    blockers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        refs::Oid,
        status::{Head, Worktree},
    };

    fn oid() -> Oid {
        Oid::parse("b87d11011148b979094156442b1e1d8d9dbed5ff").expect("valid")
    }

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    fn on(name: &str, worktree: Worktree, in_progress: Option<InProgress>) -> RepoStatus {
        RepoStatus {
            head: Head::Branch {
                name: branch(name),
                oid: oid(),
                upstream: None,
            },
            worktree,
            in_progress,
        }
    }

    #[test]
    fn a_clean_branch_blocks_nothing() {
        let status = on("main", Worktree::default(), None);
        assert!(
            blockers(
                Operation::Rebase,
                &status,
                Some(&branch("main")),
                Autostash::Disabled
            )
            .is_empty()
        );
    }

    /// Autostash is the answer to a dirty worktree, so ticking the box has to
    /// clear that blocker — otherwise the checkbox could never be used.
    #[test]
    fn autostash_clears_a_dirty_worktree() {
        let dirty = Worktree {
            staged: 1,
            unstaged: 2,
            ..Worktree::default()
        };
        let status = on("main", dirty, None);

        assert_eq!(
            blockers(Operation::Rebase, &status, None, Autostash::Disabled),
            vec![Blocker::DirtyWorktree {
                staged: 1,
                unstaged: 2
            }]
        );
        assert!(blockers(Operation::Rebase, &status, None, Autostash::Enabled).is_empty());
    }

    /// Autostash clears dirt, not a half-finished operation. Stashing on top of
    /// an in-progress rebase would bury it rather than get out of its way.
    #[test]
    fn autostash_does_not_clear_an_operation_already_in_progress() {
        let status = on("main", Worktree::default(), Some(InProgress::Rebase));
        for autostash in [Autostash::Enabled, Autostash::Disabled] {
            assert_eq!(
                blockers(Operation::Rebase, &status, None, autostash),
                vec![Blocker::InProgress(InProgress::Rebase)],
                "{autostash:?}"
            );
        }
    }

    /// An autostash pop that conflicted leaves unmerged paths and no sequencer
    /// state, and autostash cannot help with that either.
    #[test]
    fn autostash_does_not_clear_unresolved_conflicts() {
        let status = on(
            "main",
            Worktree {
                conflicted: 3,
                ..Worktree::default()
            },
            None,
        );
        assert_eq!(
            blockers(Operation::Merge, &status, None, Autostash::Enabled),
            vec![Blocker::UnresolvedConflicts { count: 3 }]
        );
    }

    #[test]
    fn a_detached_or_unborn_head_has_nothing_to_rebase() {
        let detached = RepoStatus {
            head: Head::Detached { oid: oid() },
            worktree: Worktree::default(),
            in_progress: None,
        };
        assert_eq!(
            blockers(Operation::Rebase, &detached, None, Autostash::Enabled),
            vec![Blocker::DetachedHead { oid: oid() }]
        );

        let unborn = RepoStatus {
            head: Head::Unborn {
                name: branch("main"),
            },
            worktree: Worktree::default(),
            in_progress: None,
        };
        assert_eq!(
            blockers(Operation::Rebase, &unborn, None, Autostash::Enabled),
            vec![Blocker::UnbornBranch {
                name: branch("main")
            }]
        );
    }

    /// The one-argument `rebase` and `merge` forms never check anything out, so
    /// being on the wrong branch is a refusal, not something to fix silently.
    #[test]
    fn being_on_a_different_branch_is_a_blocker() {
        let status = on("other", Worktree::default(), None);
        assert_eq!(
            blockers(
                Operation::Merge,
                &status,
                Some(&branch("main")),
                Autostash::Enabled
            ),
            vec![Blocker::WrongBranch {
                expected: branch("main"),
                actual: branch("other"),
            }]
        );
    }

    /// A fetch writes only `refs/remotes/`, so none of the worktree's problems
    /// are its problem. Greying out refresh because a file is edited would be a
    /// bug users notice immediately.
    #[test]
    fn nothing_about_the_worktree_blocks_a_fetch() {
        let status = on(
            "main",
            Worktree {
                staged: 4,
                unstaged: 5,
                conflicted: 6,
                untracked: 7,
            },
            Some(InProgress::Rebase),
        );
        assert!(blockers(Operation::Fetch, &status, None, Autostash::Disabled).is_empty());
        assert!(blockers(Operation::Abort, &status, None, Autostash::Disabled).is_empty());
    }

    /// The most fundamental cause is reported first, so a single-line tooltip
    /// says "a rebase is in progress" rather than "3 paths conflict".
    #[test]
    fn the_most_fundamental_blocker_is_reported_first() {
        let status = RepoStatus {
            head: Head::Detached { oid: oid() },
            worktree: Worktree {
                staged: 1,
                conflicted: 2,
                ..Worktree::default()
            },
            in_progress: Some(InProgress::Rebase),
        };
        let found = blockers(Operation::Rebase, &status, None, Autostash::Disabled);
        assert_eq!(found.len(), 4, "{found:?}");
        assert_eq!(found[0], Blocker::InProgress(InProgress::Rebase));

        let preflight = Preflight::new(Operation::Rebase, found);
        assert!(!preflight.is_clear());
        assert_eq!(
            preflight.reason().as_deref(),
            Some("a rebase is in progress")
        );
    }

    #[test]
    fn a_clear_preflight_has_no_reason() {
        let preflight = Preflight::new(Operation::Rebase, Vec::new());
        assert!(preflight.is_clear());
        assert_eq!(preflight.reason(), None);
        assert_eq!(preflight.operation(), Operation::Rebase);
    }

    /// The flag is always spelled out; there is no "leave it to git" value.
    #[test]
    fn autostash_always_names_a_flag() {
        assert_eq!(Autostash::Enabled.as_flag(), "--autostash");
        assert_eq!(Autostash::Disabled.as_flag(), "--no-autostash");
    }
}
