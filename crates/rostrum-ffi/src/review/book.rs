//! The pending review of one pull request, held in memory with ids.
//!
//! `rostrum-db` stores a pull request's drafts as one set tagged with the
//! head commit they were anchored against. This wraps that set with ids that
//! stay stable while the process lives, so Kotlin can edit or remove one
//! draft without addressing it by a position that shifts.
//!
//! Invariant: every draft in a book was anchored against the same head. The
//! first draft tags the book; a stale book refuses new drafts; an emptied book
//! forgets its tag.

use rostrum_core::{DraftAnchor as CoreAnchor, Side as CoreSide, drafts_are_stale};
use rostrum_db::DraftSet;
use rostrum_github::DraftComment;

use crate::{
    engine::{state::PullKey, writer::Write},
    error::RostrumError,
    review::{DraftAnchor, PendingReview, ReviewDraft},
};

/// One draft and its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Draft {
    pub id: u64,
    pub comment: DraftComment,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DraftBook {
    /// The head the drafts were anchored against; `None` exactly when empty.
    head_sha: Option<String>,
    drafts: Vec<Draft>,
}

impl DraftBook {
    /// Adopt what SQLite held, numbering the drafts from `next_id`.
    pub(crate) fn from_set(set: Option<DraftSet>, next_id: &mut u64) -> Self {
        let Some(set) = set.filter(|set| !set.comments.is_empty()) else {
            return Self::default();
        };
        let drafts = set
            .comments
            .into_iter()
            .map(|comment| {
                let id = *next_id;
                *next_id += 1;
                Draft { id, comment }
            })
            .collect();
        Self {
            head_sha: Some(set.head_sha),
            drafts,
        }
    }

    pub(crate) fn drafts(&self) -> &[Draft] {
        &self.drafts
    }

    pub(crate) fn is_stale(&self, head: &str) -> bool {
        drafts_are_stale(self.head_sha.as_deref(), head)
    }

    /// Refuse when the drafts were anchored against another head.
    pub(crate) fn ensure_fresh(&self, head: &str) -> Result<(), RostrumError> {
        match &self.head_sha {
            Some(drafted) if self.is_stale(head) => Err(RostrumError::DraftsStale {
                drafted_against: drafted.clone(),
                head: head.to_string(),
            }),
            _ => Ok(()),
        }
    }

    /// Add a draft anchored against `head`. The first draft tags the book.
    pub(crate) fn add(
        &mut self,
        id: u64,
        anchor: &CoreAnchor,
        body: &str,
        head: &str,
    ) -> Result<(), RostrumError> {
        let body = non_blank(body)?;
        self.ensure_fresh(head)?;
        if self.drafts.is_empty() {
            self.head_sha = Some(head.to_string());
        }
        self.drafts.push(Draft {
            id,
            comment: DraftComment {
                path: anchor.path.clone(),
                line: anchor.line,
                side: anchor.side,
                start_line: anchor.start_line,
                start_side: anchor.start_side,
                body,
            },
        });
        Ok(())
    }

    /// Replace a draft's text. Allowed on a stale book: the text does not
    /// depend on the anchor.
    pub(crate) fn edit(&mut self, id: u64, body: &str) -> Result<(), RostrumError> {
        let body = non_blank(body)?;
        let draft = self
            .drafts
            .iter_mut()
            .find(|draft| draft.id == id)
            .ok_or_else(|| unknown(id))?;
        draft.comment.body = body;
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: u64) -> Result<(), RostrumError> {
        let before = self.drafts.len();
        self.drafts.retain(|draft| draft.id != id);
        if self.drafts.len() == before {
            return Err(unknown(id));
        }
        self.forget_head_if_empty();
        Ok(())
    }

    /// Drop the drafts with these ids — the ones a submitted review carried,
    /// leaving any added while it was in flight.
    pub(crate) fn remove_ids(&mut self, ids: &[u64]) {
        self.drafts.retain(|draft| !ids.contains(&draft.id));
        self.forget_head_if_empty();
    }

    pub(crate) fn clear(&mut self) {
        self.drafts.clear();
        self.head_sha = None;
    }

    /// The write that makes SQLite match this book.
    pub(crate) fn write(&self, key: &PullKey) -> Write {
        Write::Drafts {
            repo: key.repo.clone(),
            number: key.number,
            head_sha: self.head_sha.clone().unwrap_or_default(),
            drafts: self.drafts.iter().map(|draft| draft.comment.clone()).collect(),
        }
    }

    /// The pending review as Kotlin shows it, checked against `head`.
    pub(crate) fn view(&self, key: &PullKey, head: &str) -> PendingReview {
        PendingReview {
            repo: key.repo.to_string(),
            number: key.number.0,
            drafts: self.drafts.iter().map(review_draft).collect(),
            drafted_against: self.head_sha.clone(),
            head_sha: head.to_string(),
            stale: self.is_stale(head),
        }
    }

    fn forget_head_if_empty(&mut self) {
        if self.drafts.is_empty() {
            self.head_sha = None;
        }
    }
}

pub(crate) fn review_draft(draft: &Draft) -> ReviewDraft {
    let comment = &draft.comment;
    ReviewDraft {
        id: draft.id,
        anchor: DraftAnchor {
            path: comment.path.clone(),
            line: comment.line,
            side: comment.side.into(),
            start_line: comment.start_line,
        },
        body: comment.body.clone(),
        location: location(&comment.path, comment.start_line, comment.line, comment.side),
    }
}

/// `src/main.rs:12`, `src/main.rs lines 12–18`, and `(old)` for the left side.
fn location(path: &str, start: Option<u32>, end: u32, side: CoreSide) -> String {
    let lines = match start {
        Some(start) if start != end => format!(" lines {start}–{end}"),
        _ => format!(":{end}"),
    };
    let old = if side == CoreSide::Left { " (old)" } else { "" };
    format!("{path}{lines}{old}")
}

fn non_blank(body: &str) -> Result<String, RostrumError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(RostrumError::invalid("a comment needs some text"));
    }
    Ok(body.to_string())
}

fn unknown(id: u64) -> RostrumError {
    RostrumError::invalid(format!("there is no draft {id}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rostrum_core::PrNumber;

    use super::*;

    fn key() -> PullKey {
        PullKey {
            repo: "a/b".parse().expect("repo"),
            number: PrNumber(4),
        }
    }

    fn anchor(line: u32) -> CoreAnchor {
        CoreAnchor::single("src/main.rs", line, CoreSide::Right)
    }

    #[test]
    fn the_first_draft_tags_the_book_with_the_head() {
        let mut book = DraftBook::default();
        book.add(1, &anchor(3), "first", "aaa").expect("add");
        book.add(2, &anchor(5), "second", "aaa").expect("add");
        let view = book.view(&key(), "aaa");
        assert_eq!(view.drafted_against.as_deref(), Some("aaa"));
        assert!(!view.stale);
        assert_eq!(view.drafts.len(), 2);
        assert_eq!(view.drafts[0].location, "src/main.rs:3");
    }

    #[test]
    fn a_moved_head_makes_the_book_stale_and_refuses_additions() {
        let mut book = DraftBook::default();
        book.add(1, &anchor(3), "first", "aaa").expect("add");
        assert!(book.view(&key(), "bbb").stale);
        assert_eq!(
            book.add(2, &anchor(4), "more", "bbb"),
            Err(RostrumError::DraftsStale {
                drafted_against: "aaa".into(),
                head: "bbb".into()
            })
        );
        // Text edits and removal still work on a stale book.
        book.edit(1, "reworded").expect("edit");
        book.remove(1).expect("remove");
        // Emptied, it forgets the old head and accepts drafts for the new one.
        book.add(3, &anchor(4), "fresh", "bbb").expect("add");
        assert!(!book.view(&key(), "bbb").stale);
    }

    #[test]
    fn edits_and_removals_address_drafts_by_id() {
        let mut book = DraftBook::default();
        book.add(7, &anchor(1), "one", "h").expect("add");
        book.add(9, &anchor(2), "two", "h").expect("add");
        book.remove(7).expect("remove");
        book.edit(9, "  two, reworded ").expect("edit");
        let view = book.view(&key(), "h");
        assert_eq!(view.drafts.len(), 1);
        assert_eq!(view.drafts[0].id, 9);
        assert_eq!(view.drafts[0].body, "two, reworded");
        assert!(matches!(book.edit(7, "gone"), Err(RostrumError::InvalidInput { .. })));
        assert!(matches!(book.remove(7), Err(RostrumError::InvalidInput { .. })));
    }

    #[test]
    fn blank_text_is_refused() {
        let mut book = DraftBook::default();
        assert!(matches!(
            book.add(1, &anchor(1), "  \n", "h"),
            Err(RostrumError::InvalidInput { .. })
        ));
        book.add(1, &anchor(1), "ok", "h").expect("add");
        assert!(matches!(book.edit(1, ""), Err(RostrumError::InvalidInput { .. })));
    }

    #[test]
    fn a_range_keeps_its_start_and_describes_itself() {
        let mut book = DraftBook::default();
        let range = CoreAnchor::single("a.rs", 10, CoreSide::Left).extended_to(14, CoreSide::Left);
        book.add(1, &range, "range", "h").expect("add");
        let draft = &book.view(&key(), "h").drafts[0];
        assert_eq!(draft.anchor.start_line, Some(10));
        assert_eq!(draft.anchor.line, 14);
        assert_eq!(draft.location, "a.rs lines 10–14 (old)");
        let Write::Drafts { drafts, .. } = book.write(&key()) else {
            panic!("a drafts write");
        };
        assert_eq!(drafts[0].start_side, Some(CoreSide::Left));
    }

    #[test]
    fn removing_submitted_ids_keeps_later_drafts() {
        let mut book = DraftBook::default();
        book.add(1, &anchor(1), "one", "h").expect("add");
        book.add(2, &anchor(2), "two", "h").expect("add");
        book.remove_ids(&[1]);
        assert_eq!(book.drafts().len(), 1);
        book.remove_ids(&[2]);
        assert_eq!(book.view(&key(), "h").drafted_against, None);
    }

    #[test]
    fn a_stored_set_is_adopted_with_fresh_ids() {
        let mut next = 5;
        let set = DraftSet {
            head_sha: "h".into(),
            comments: vec![
                DraftComment::single("a.rs", 1, CoreSide::Right, "x"),
                DraftComment::single("a.rs", 2, CoreSide::Right, "y"),
            ],
            updated_at: Utc::now(),
        };
        let book = DraftBook::from_set(Some(set), &mut next);
        assert_eq!(
            book.drafts().iter().map(|d| d.id).collect::<Vec<_>>(),
            vec![5, 6]
        );
        assert_eq!(next, 7);
        assert!(DraftBook::from_set(None, &mut next).drafts().is_empty());
    }

    #[test]
    fn an_emptied_book_writes_a_clear() {
        let mut book = DraftBook::default();
        book.add(1, &anchor(1), "one", "h").expect("add");
        book.clear();
        let Write::Drafts { drafts, head_sha, .. } = book.write(&key()) else {
            panic!("a drafts write");
        };
        assert!(drafts.is_empty());
        assert!(head_sha.is_empty());
    }
}
