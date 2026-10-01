//! Inline comments on GitHub's pending-review model, held on the phone and
//! persisted in SQLite until submitted, so a crash loses nothing.

mod anchor;
pub(crate) mod book;
mod types;

pub(crate) use book::DraftBook;
pub use types::{DraftAnchor, PendingReview, ReviewDraft, ReviewEvent};

use rostrum_github::{ReviewEvent as GitHubEvent, SubmitReview};

use crate::{
    diff::CommentAnchor,
    engine::{
        RostrumCore,
        state::{CoreState, PullKey},
        writer::{Ack, settled},
    },
    error::RostrumError,
    review::book::Draft,
};

impl CoreState {
    /// The current head of `key`, which drafts are checked against.
    fn head_of(&self, key: &PullKey) -> Result<String, RostrumError> {
        Ok(self.known(key)?.head_sha.clone())
    }

    /// Apply `change` to `key`'s loaded book, queue the write that persists
    /// it, and return the review as it now stands with the write's ack.
    fn change_book(
        &mut self,
        key: &PullKey,
        change: impl FnOnce(&mut DraftBook, &str, &mut u64) -> Result<(), RostrumError>,
    ) -> Result<(PendingReview, Ack), RostrumError> {
        let head = self.head_of(key)?;
        let book = self
            .drafts
            .get_mut(key)
            .ok_or_else(|| RostrumError::internal("pending review not loaded"))?;
        change(book, &head, &mut self.next_draft_id)?;
        let ack = self.writer.send_acked(book.write(key));
        Ok((book.view(key, &head), ack))
    }
}

/// Whether GitHub will accept a review. An approval may be empty; a comment
/// or a change request needs a body or inline comments.
fn check_review(event: ReviewEvent, body: &str, comments: usize) -> Result<(), RostrumError> {
    if event == ReviewEvent::Approve || !body.trim().is_empty() || comments > 0 {
        return Ok(());
    }
    Err(RostrumError::invalid(
        "write a comment or add inline feedback first",
    ))
}

impl From<ReviewEvent> for GitHubEvent {
    fn from(event: ReviewEvent) -> Self {
        match event {
            ReviewEvent::Comment => Self::Comment,
            ReviewEvent::Approve => Self::Approve,
            ReviewEvent::RequestChanges => Self::RequestChanges,
        }
    }
}

impl RostrumCore {
    /// Read `key`'s pending review from SQLite into memory, once. A draft
    /// that fails to decode is an error, never a silent discard: it is the
    /// user's unsent work.
    pub(crate) async fn ensure_book(&self, key: &PullKey) -> Result<(), RostrumError> {
        let lookup = key.clone();
        if self
            .actor
            .call(move |state| state.drafts.contains_key(&lookup))
            .await?
        {
            return Ok(());
        }
        let stored = self.db.load_drafts(&key.repo, key.number).await?;
        let store = key.clone();
        self.actor
            .call(move |state| {
                if !state.drafts.contains_key(&store) {
                    let book = DraftBook::from_set(stored, &mut state.next_draft_id);
                    state.drafts.insert(store, book);
                }
            })
            .await
    }

    /// The pending review of `key`, checked against its current head.
    pub(crate) async fn pending(&self, key: &PullKey) -> Result<PendingReview, RostrumError> {
        self.ensure_hydrated().await?;
        self.ensure_book(key).await?;
        let lookup = key.clone();
        self.actor
            .try_call(move |state| {
                let head = state.head_of(&lookup)?;
                let book = state.drafts.get(&lookup).cloned().unwrap_or_default();
                Ok(book.view(&lookup, &head))
            })
            .await
    }

    /// Drafts to place in the diff: none while they are stale, since their
    /// anchors may no longer name the lines they were written on.
    pub(crate) async fn draft_list(&self, key: &PullKey) -> Result<Vec<Draft>, RostrumError> {
        self.ensure_book(key).await?;
        let lookup = key.clone();
        self.actor
            .try_call(move |state| {
                let head = state.head_of(&lookup)?;
                Ok(state
                    .drafts
                    .get(&lookup)
                    .filter(|book| !book.is_stale(&head))
                    .map(|book| book.drafts().to_vec())
                    .unwrap_or_default())
            })
            .await
    }

    async fn change_drafts(
        &self,
        key: PullKey,
        change: impl FnOnce(&mut DraftBook, &str, &mut u64) -> Result<(), RostrumError> + Send + 'static,
    ) -> Result<PendingReview, RostrumError> {
        self.ensure_hydrated().await?;
        self.ensure_book(&key).await?;
        let (review, ack) = self
            .actor
            .try_call(move |state| state.change_book(&key, change))
            .await?;
        settled(ack).await?;
        Ok(review)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The pending review, with staleness checked against the current head.
    pub async fn pending_review(
        &self,
        repo: String,
        number: u32,
    ) -> Result<PendingReview, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.pending(&key).await
    }

    /// Add an inline comment. `anchor` is a `DiffLineView::anchor` from
    /// `file_diff`; for a multi-line comment, `range_start` is the anchor of
    /// the first line (same file, same side, same hunk). Anchors are checked
    /// against the diff of the current head; one that is not there is
    /// `InvalidInput`. The first draft tags the review with the head commit.
    /// Returns once the draft is on disk.
    pub async fn add_draft(
        &self,
        repo: String,
        number: u32,
        anchor: CommentAnchor,
        range_start: Option<CommentAnchor>,
        body: String,
    ) -> Result<PendingReview, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let files = self.load_files(&key).await?;
        let resolved = anchor::resolve(&files.files, &anchor, range_start.as_ref())?;
        let diff_head = files.head_sha.clone();
        self.change_drafts(key, move |book, head, next_id| {
            // The feed may have moved the head between reading the diff and
            // getting here; the anchor was checked against the old one.
            if diff_head != head {
                return Err(RostrumError::DraftsStale {
                    drafted_against: diff_head,
                    head: head.to_string(),
                });
            }
            let id = *next_id;
            book.add(id, &resolved, &body, head)?;
            *next_id += 1;
            Ok(())
        })
        .await
    }

    /// Replace a draft's text. Returns once the change is on disk.
    pub async fn edit_draft(
        &self,
        repo: String,
        number: u32,
        draft_id: u64,
        body: String,
    ) -> Result<PendingReview, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.change_drafts(key, move |book, _, _| book.edit(draft_id, &body))
            .await
    }

    pub async fn remove_draft(
        &self,
        repo: String,
        number: u32,
        draft_id: u64,
    ) -> Result<PendingReview, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.change_drafts(key, move |book, _, _| book.remove(draft_id))
            .await
    }

    /// Throw the whole pending review away.
    pub async fn discard_drafts(
        &self,
        repo: String,
        number: u32,
    ) -> Result<PendingReview, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.change_drafts(key, |book, _, _| {
            book.clear();
            Ok(())
        })
        .await
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
        let key = PullKey::parse(&repo, number)?;
        self.ensure_hydrated().await?;
        self.ensure_book(&key).await?;
        let lookup = key.clone();
        let (client, drafts) = self
            .actor
            .try_call(move |state| {
                let client = state.github()?;
                let head = state.head_of(&lookup)?;
                let drafts = match state.drafts.get(&lookup) {
                    Some(book) if include_drafts => {
                        book.ensure_fresh(&head)?;
                        book.drafts().to_vec()
                    }
                    _ => Vec::new(),
                };
                Ok((client, drafts))
            })
            .await?;
        check_review(event, &body, drafts.len())?;

        let review = SubmitReview::new(event.into(), body.trim())
            .with_comments(drafts.iter().map(|draft| draft.comment.clone()).collect());
        self.github(client.submit_review(&key.repo, key.number, review).await)
            .await?;
        tracing::info!(repo = %key.repo, number = key.number.0, ?event, drafts = drafts.len(), "review submitted");

        if !drafts.is_empty() {
            let submitted: Vec<u64> = drafts.iter().map(|draft| draft.id).collect();
            self.change_drafts(key.clone(), move |book, _, _| {
                book.remove_ids(&submitted);
                Ok(())
            })
            .await?;
        }
        self.after_mutation(&key).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_approval_may_be_empty_but_nothing_else_may() {
        assert!(check_review(ReviewEvent::Approve, "", 0).is_ok());
        assert!(check_review(ReviewEvent::Comment, "  ", 0).is_err());
        assert!(check_review(ReviewEvent::RequestChanges, "", 0).is_err());
        assert!(check_review(ReviewEvent::Comment, "", 2).is_ok());
        assert!(check_review(ReviewEvent::RequestChanges, "fix this", 0).is_ok());
    }

    #[test]
    fn review_events_map_onto_githubs() {
        assert_eq!(
            GitHubEvent::from(ReviewEvent::Approve),
            GitHubEvent::Approve
        );
        assert_eq!(
            GitHubEvent::from(ReviewEvent::RequestChanges),
            GitHubEvent::RequestChanges
        );
        assert_eq!(
            GitHubEvent::from(ReviewEvent::Comment),
            GitHubEvent::Comment
        );
    }
}
