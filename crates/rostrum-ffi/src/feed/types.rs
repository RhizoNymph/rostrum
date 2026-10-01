//! The feed as Kotlin renders it: repositories in order, each with its pull
//! requests already filtered.

use std::time::SystemTime;

use crate::{
    issues::IssueSummary,
    sort::SortSettings,
    stacks::PullItem,
    types::{CheckState, Chip, ColorRole, LabelView, MergeStatus, ReviewDecision, UserRef},
};

/// One render of the feed. Every call that changes the feed returns a fresh
/// one; so does every background update, through [`FeedObserver`].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FeedSnapshot {
    /// Increases with every change. When snapshots can arrive from more than
    /// one place, keep the one with the highest revision.
    pub revision: u64,
    /// Which list the feed shows. Persisted.
    pub tab: FeedTab,
    /// What each tab's badge says: items the filter lets through, collapsed
    /// repositories included.
    pub tab_counts: TabCounts,
    /// Both sorts and every choice for each.
    pub sort: SortSettings,
    /// Watched repositories in the repository sort, minus any hidden by
    /// `hide_empty_repos` (on the active tab).
    pub repos: Vec<RepoSection>,
    /// How many repositories `hide_empty_repos` removed, for "3 empty hidden".
    pub hidden_empty_repos: u32,
    /// Open items of the active tab across every repository, before filtering.
    pub total_open: u32,
    /// Items of the active tab the filter lets through, collapsed
    /// repositories included.
    pub visible_open: u32,
    /// The search box. Not persisted.
    pub query: String,
    /// The persisted part of the filter.
    pub preferences: FeedPreferences,
    /// Whether anything narrows the feed (query, drafts hidden, authors).
    pub filter_active: bool,
    /// Whether GitHub is still computing merge states that a background
    /// re-check will fill in; a later snapshot will carry them.
    pub merge_states_settling: bool,
    /// Who the GitHub token belongs to, once a refresh has said.
    pub viewer: Option<UserRef>,
}

/// The feed filter's standing preferences, persisted in the settings file.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FeedPreferences {
    pub hide_drafts: bool,
    /// Drop repositories that loaded with nothing to show. Loading and failed
    /// repositories are never hidden.
    pub hide_empty_repos: bool,
    /// Logins the feed is narrowed to (lowercase); empty means everyone.
    pub authors: Vec<String>,
    /// Widen `authors` from "opened by" to "opened by, assigned to, or
    /// awaiting review from".
    pub include_involved: bool,
}

/// One repository's container in the feed.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RepoSection {
    /// `owner/name`.
    pub repo: String,
    /// The active tab's last fetch (pull requests and issues load
    /// separately), for the header's spinner or error mark.
    pub load: RepoLoad,
    /// Open items of the active tab in this repository, before filtering.
    pub open_count: u32,
    /// Items of the active tab the filter lets through, whether or not
    /// collapsed.
    pub visible_count: u32,
    pub collapsed: bool,
    /// What goes under the header.
    pub body: RepoBody,
}

/// A repository's fetch state.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RepoLoad {
    /// Watched but never fetched.
    Idle,
    Loading,
    Loaded {
        at: SystemTime,
    },
    /// The last fetch failed. Older pull requests may still be shown.
    Failed {
        reason: String,
        at: SystemTime,
    },
}

/// What a repository's container shows below its header.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum RepoBody {
    Collapsed,
    /// First load in flight, nothing cached.
    Loading,
    /// The fetch failed and there is nothing cached to show instead.
    Failed {
        reason: String,
    },
    /// Loaded; no open items, or none the filter lets through.
    Empty,
    /// The Pull requests tab: lone pull requests and stacks, in the item
    /// sort, a stack sorting as one unit.
    Pulls {
        items: Vec<PullItem>,
    },
    /// The Issues tab, in the item sort.
    Issues {
        issues: Vec<IssueSummary>,
    },
}

/// The feed's two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FeedTab {
    PullRequests,
    Issues,
}

/// Each tab's badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TabCounts {
    pub pull_requests: u32,
    pub issues: u32,
}

impl From<rostrum_core::FeedTab> for FeedTab {
    fn from(tab: rostrum_core::FeedTab) -> Self {
        match tab {
            rostrum_core::FeedTab::PullRequests => Self::PullRequests,
            rostrum_core::FeedTab::Issues => Self::Issues,
        }
    }
}

impl From<FeedTab> for rostrum_core::FeedTab {
    fn from(tab: FeedTab) -> Self {
        match tab {
            FeedTab::PullRequests => Self::PullRequests,
            FeedTab::Issues => Self::Issues,
        }
    }
}

/// Everything a feed row shows for one pull request.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PrSummary {
    /// `owner/name`.
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    pub author: Option<UserRef>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub is_draft: bool,
    /// CI rollup of the head commit; `None` when nothing reported.
    pub checks: Option<CheckState>,
    /// Colour for the CI dot.
    pub checks_role: ColorRole,
    pub review_decision: Option<ReviewDecision>,
    /// `approved` or `changes`; nothing while a review is merely required.
    pub review_chip: Option<Chip>,
    pub merge_status: MergeStatus,
    /// `conflict`, `behind` or `blocked`. Suppressed for `behind` when
    /// `behind_chip` carries the exact count instead.
    pub merge_chip: Option<Chip>,
    /// Distance from the base branch, once the batched compare has answered.
    /// `None` for cross-fork pull requests and before the first answer.
    pub base_divergence: Option<BaseDivergence>,
    /// `↓N` when the branch is behind its base.
    pub behind_chip: Option<Chip>,
    pub labels: Vec<LabelView>,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub comment_count: u32,
    /// Your review is requested on it.
    pub review_requested: bool,
    /// You opened it.
    pub is_yours: bool,
    pub head_ref: String,
    pub base_ref: String,
}

/// How far a branch has drifted from its base.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BaseDivergence {
    /// Commits on the base this branch lacks.
    pub behind: u32,
    /// Commits on this branch the base lacks.
    pub ahead: u32,
    pub base_ref: String,
    /// Behind with no commits of its own: merge and rebase give the same
    /// result.
    pub fast_forwards: bool,
    /// `3 commit(s) behind main, 2 ahead`.
    pub summary: String,
}

/// The author filter's chips, capped for display.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AuthorRoster {
    /// You first, then everyone else by most recent activity. Selected
    /// authors are never cut by the cap.
    pub authors: Vec<AuthorChip>,
    /// How many the cap left out, for "+N more".
    pub hidden: u32,
}

/// One person the feed can be narrowed to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AuthorChip {
    /// Display casing, as GitHub returned it.
    pub login: String,
    pub avatar_url: Option<String>,
    /// Open items of the active tab they authored across the feed.
    pub open_items: u32,
    pub is_viewer: bool,
    pub selected: bool,
}

/// Receives every feed change, including ones made in the background (merge
/// states GitHub finished computing after a refresh). Called on a background
/// thread, in revision order; keep it short, e.g. emit into a `StateFlow`.
#[uniffi::export(with_foreign)]
pub trait FeedObserver: Send + Sync {
    fn feed_changed(&self, snapshot: FeedSnapshot);
}
