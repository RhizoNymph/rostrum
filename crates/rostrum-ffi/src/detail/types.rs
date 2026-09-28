//! The pull request detail screen: header, conversation, threads, checks.

use std::time::SystemTime;

use crate::{
    feed::BaseDivergence,
    markdown::MdBlock,
    review::PendingReview,
    types::{
        CheckState, Chip, ColorRole, LabelView, MergeStatus, PullState, ReviewDecision,
        ReviewState, Side, UserRef,
    },
};

/// Everything the Conversation, Checks and Branch tabs show.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PullDetail {
    pub header: PullHeader,
    /// Description first, then comments, reviews and events, oldest first.
    pub timeline: Vec<TimelineEntry>,
    /// Inline review threads, stored once; reviews reference them by id and
    /// the diff shows them at their lines.
    pub threads: Vec<ReviewThreadView>,
    pub checks: Vec<CheckRunView>,
    pub unresolved_threads: u32,
    /// Your unsent inline comments on this pull request.
    pub pending_review: PendingReview,
}

/// The header, and the facts the Branch tab and the action bar decide on.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PullHeader {
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    pub state: PullState,
    pub is_draft: bool,
    pub author: Option<UserRef>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub head_ref: String,
    pub base_ref: String,
    /// The commit the pull request points at. Pass it back as the expected
    /// head to `merge` and `update_branch`, so a push made after this was
    /// rendered is refused rather than acted on blind.
    pub head_sha: String,
    pub labels: Vec<LabelView>,
    pub assignees: Vec<UserRef>,
    /// People (not teams) whose review is outstanding.
    pub review_requests: Vec<UserRef>,
    pub review_decision: Option<ReviewDecision>,
    pub review_chip: Option<Chip>,
    pub merge: MergeVerdict,
    pub divergence: Option<BaseDivergence>,
    pub checks: Option<CheckState>,
    pub checks_role: ColorRole,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub comment_count: u32,
    pub is_yours: bool,
    pub review_requested: bool,
    /// The draft button: the state it moves to, fixed now so a refresh
    /// landing before the tap can never invert it.
    pub draft_action: DraftAction,
}

/// Can this be merged, and if not, why — in one place so the chip, the
/// disabled button and its explanation agree.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MergeVerdict {
    pub status: MergeStatus,
    /// "Blocked by branch protection: a required review or check is missing".
    pub sentence: String,
    /// Disable the merge button.
    pub blocks_merge: bool,
    /// Colour of the verdict's dot.
    pub role: ColorRole,
    /// The short chip, for states worth one.
    pub chip: Option<Chip>,
}

/// What the draft toggle does when tapped.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DraftAction {
    /// Pass to `set_draft`: `true` converts to draft, `false` marks ready.
    pub to_draft: bool,
    /// "Convert to draft" or "Ready for review".
    pub label: String,
}

/// One item of the conversation.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TimelineEntry {
    /// Stable key for list diffing.
    pub id: String,
    pub author: Option<UserRef>,
    pub created_at: SystemTime,
    pub kind: TimelineKind,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum TimelineKind {
    /// The pull request's description; always first.
    Description {
        body: Vec<MdBlock>,
        /// The raw markdown, for quoting.
        source: String,
    },
    Comment {
        body: Vec<MdBlock>,
        source: String,
    },
    Review {
        state: ReviewState,
        /// `approved`, `requested changes`, `reviewed`, …
        chip: Chip,
        body: Vec<MdBlock>,
        source: String,
        /// Threads this review opened, by `ReviewThreadView::id`.
        thread_ids: Vec<String>,
    },
    Event {
        event: TimelineEvent,
        /// "renamed this from “a” to “b”", to follow the actor's login.
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineEvent {
    Merged,
    Closed,
    Reopened,
    ReadyForReview,
    ConvertedToDraft,
    ForcePushed,
    ReviewRequested { reviewer: String },
    Assigned { assignee: String },
    Labeled { label: String },
    Unlabeled { label: String },
    Renamed { from: String, to: String },
    Other { kind: String },
}

/// An inline review thread.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReviewThreadView {
    pub id: String,
    pub path: String,
    /// Line in the current diff; `None` once outdated.
    pub line: Option<u32>,
    pub original_line: Option<u32>,
    pub side: Side,
    pub resolved: bool,
    pub outdated: bool,
    /// `src/main.rs:12`, or `src/main.rs (outdated)`.
    pub location: String,
    pub comments: Vec<ThreadCommentView>,
    /// Whether `reply_to_thread` can reply here (GitHub gave a comment id).
    pub can_reply: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ThreadCommentView {
    pub id: String,
    pub author: Option<UserRef>,
    pub created_at: SystemTime,
    pub body: Vec<MdBlock>,
    pub source: String,
}

/// One CI check on the head commit.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CheckRunView {
    pub name: String,
    pub state: Option<CheckState>,
    pub role: ColorRole,
    /// `success`, `failure`, … or `no status`.
    pub status_text: String,
    pub url: Option<String>,
}

/// How GitHub should merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

/// How to bring a branch up to date with its base, on GitHub's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BranchUpdateMethod {
    /// Merge the base into the branch.
    Merge,
    /// Replay the branch's commits onto the base.
    Rebase,
}
