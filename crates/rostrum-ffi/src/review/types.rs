//! The pending review: inline comments held on the phone until submitted.

use crate::types::Side;

/// Your unsent inline comments on one pull request.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PendingReview {
    pub repo: String,
    pub number: u32,
    pub drafts: Vec<ReviewDraft>,
    /// The head commit the drafts were anchored against; `None` when there
    /// are none.
    pub drafted_against: Option<String>,
    /// The pull request's current head commit.
    pub head_sha: String,
    /// The head moved after the drafts were written, so their anchors may
    /// point at the wrong lines. Stale drafts cannot be submitted or added
    /// to; discard them and re-read the diff.
    pub stale: bool,
}

/// One pending inline comment.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReviewDraft {
    /// Stable for the life of the process; pass to `edit_draft` and
    /// `remove_draft`.
    pub id: u64,
    pub anchor: DraftAnchor,
    pub body: String,
    /// `src/main.rs:12` or `src/main.rs lines 12–18`.
    pub location: String,
}

/// Where a draft attaches. A range spans `start_line..=line` on one side.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DraftAnchor {
    pub path: String,
    pub line: u32,
    pub side: Side,
    /// First line of a multi-line comment; `None` for a single line.
    pub start_line: Option<u32>,
}

/// The verdict a review is submitted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}
