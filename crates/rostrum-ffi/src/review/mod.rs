//! Inline comments on GitHub's pending-review model, held on the phone and
//! persisted in SQLite until submitted, so a crash loses nothing.

mod types;

pub use types::{DraftAnchor, PendingReview, ReviewDraft, ReviewEvent};

use crate::{diff::CommentAnchor, engine::RostrumCore, error::RostrumError};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The pending review, with staleness checked against the current head.
    pub async fn pending_review(
        &self,
        repo: String,
        number: u32,
    ) -> Result<PendingReview, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("pending_review"))
    }

    /// Add an inline comment. `anchor` is a `DiffLineView::anchor` from
    /// `file_diff`; for a multi-line comment, `range_start` is the anchor of
    /// the first line (same file, same side, same hunk). Anchors are checked
    /// against the diff of the current head; one that is not there is
    /// `InvalidInput`. The first draft tags the review with the head commit.
    pub async fn add_draft(
        &self,
        repo: String,
        number: u32,
        anchor: CommentAnchor,
        range_start: Option<CommentAnchor>,
        body: String,
    ) -> Result<PendingReview, RostrumError> {
        let _ = (repo, number, anchor, range_start, body);
        Err(RostrumError::unimplemented("add_draft"))
    }

    /// Replace a draft's text.
    pub async fn edit_draft(
        &self,
        repo: String,
        number: u32,
        draft_id: u64,
        body: String,
    ) -> Result<PendingReview, RostrumError> {
        let _ = (repo, number, draft_id, body);
        Err(RostrumError::unimplemented("edit_draft"))
    }

    pub async fn remove_draft(
        &self,
        repo: String,
        number: u32,
        draft_id: u64,
    ) -> Result<PendingReview, RostrumError> {
        let _ = (repo, number, draft_id);
        Err(RostrumError::unimplemented("remove_draft"))
    }

    /// Throw the whole pending review away.
    pub async fn discard_drafts(
        &self,
        repo: String,
        number: u32,
    ) -> Result<PendingReview, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("discard_drafts"))
    }

    /// Submit a review. With `include_drafts`, the pending drafts go with it
    /// as inline comments and are cleared once GitHub accepts; stale drafts
    /// are refused with `DraftsStale`. A comment or change request needs a
    /// body or drafts; an approval may be empty.
    pub async fn submit_review(
        &self,
        repo: String,
        number: u32,
        event: ReviewEvent,
        body: String,
        include_drafts: bool,
    ) -> Result<(), RostrumError> {
        let _ = (repo, number, event, body, include_drafts);
        Err(RostrumError::unimplemented("submit_review"))
    }
}
