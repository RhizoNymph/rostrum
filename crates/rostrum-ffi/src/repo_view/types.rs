//! One repository's screen: its lists, and its branch tree.

use crate::{
    feed::{PrSummary, RepoLoad},
    issues::IssueSummary,
    stacks::PullItem,
};

/// A repository on its own: every open pull request and issue, unfiltered,
/// in the feed's item sort, stacks grouped as in the feed.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RepoOverview {
    /// `owner/name`.
    pub repo: String,
    /// The repository on GitHub.
    pub url: String,
    /// From the feed's repository facts, or the branch tree's; `None` until
    /// either has been fetched.
    pub stars: Option<u32>,
    /// Known once `branch_tree` has run for the repository.
    pub default_branch: Option<String>,
    pub pulls: Vec<PullItem>,
    pub issues: Vec<IssueSummary>,
    pub pulls_load: RepoLoad,
    pub issues_load: RepoLoad,
}

/// How far two branches have drifted: commits each has that the other lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct BranchDrift {
    /// Commits on this branch its reference lacks.
    pub ahead: u32,
    /// Commits on the reference this branch lacks.
    pub behind: u32,
}

/// A trunk's distance from the default branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrunkDrift {
    /// This trunk is the default branch.
    Default,
    /// Configured, but GitHub has no branch by that name.
    Missing,
    /// Exists, but no count arrived.
    Unknown,
    Known {
        drift: BranchDrift,
    },
}

/// Why a pull request sits where it does in the tree, when it is not
/// self-evident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BranchNote {
    /// Its base chain loops back to it; the loop is cut here.
    BreaksCycle,
    /// Its base is the head of more than one open pull request (forks
    /// reusing a name).
    AmbiguousBase,
}

/// One row of the branch tree, depth-first.
// As `PullItem`: UniFFI cannot carry a `Box`, and rows go straight to Kotlin.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum BranchRow {
    /// A trunk, with the pull requests anywhere beneath it.
    Trunk {
        name: String,
        drift: TrunkDrift,
        pulls: u32,
    },
    /// The heading over every `Base` group; present only when there is one.
    OtherBases,
    /// A base that is neither a trunk nor one pull request's head.
    Base { name: String, pulls: u32 },
    /// A pull request. `depth` is 1 directly under a trunk or base and grows
    /// by one per level of stacking.
    Pull {
        depth: u32,
        number: u32,
        head: String,
        base: String,
        /// Head against base; `None` when unknown (a cross-fork head).
        drift: Option<BranchDrift>,
        note: Option<BranchNote>,
        /// `stack 7` for a member of a GitHub stack, `chain` for a chain
        /// rostrum detected; as the feed groups them.
        stack_label: Option<String>,
        /// The row as the feed shows it, when the pull request is loaded.
        pull: Option<PrSummary>,
    },
}

/// Which branches are a repository's trunks.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TrunkSettings {
    /// `true`: no list is configured, and the trunks are whichever of
    /// `main`, `master`, `staging`, `develop` exist besides the default
    /// branch. `false`: exactly `configured` (empty means the default branch
    /// alone).
    pub detected: bool,
    pub configured: Vec<String>,
    /// Branch names the last branch-tree fetch found to exist among the ones
    /// it asked about, to offer as trunks. Empty before one has run.
    pub existing: Vec<String>,
}

/// The branch tree: each trunk with its distance from the default branch,
/// the pull requests based on it, and pull requests stacked on those.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BranchTree {
    pub repo: String,
    pub url: String,
    pub stars: u32,
    /// `None` for a repository with no commits; then `rows` is empty.
    pub default_branch: Option<String>,
    pub trunks: TrunkSettings,
    pub rows: Vec<BranchRow>,
}
