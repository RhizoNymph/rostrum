//! The rules of a pending review that are not about the network: where an
//! inline comment attaches, and when drafted anchors can no longer be
//! trusted.
//!
//! Shared by the desktop's diff view and the Android core, so both apps order
//! a range and detect a force-push the same way.

use crate::model::Side;

/// Where an inline comment will be attached.
///
/// `start_line`/`start_side` are set only for a multi-line selection; GitHub
/// wants them omitted entirely for a single-line comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftAnchor {
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub start_line: Option<u32>,
    pub start_side: Option<Side>,
}

impl DraftAnchor {
    pub fn single(path: impl Into<String>, line: u32, side: Side) -> Self {
        Self {
            path: path.into(),
            line,
            side,
            start_line: None,
            start_side: None,
        }
    }

    /// Extend an anchor to cover the range between it and `line`.
    ///
    /// GitHub requires `start_line <= line`, so the two are ordered here rather
    /// than trusting the click order.
    pub fn extended_to(&self, line: u32, side: Side) -> Self {
        let (start, end) = if line < self.anchor_start() {
            (line, self.line)
        } else {
            (self.anchor_start(), line)
        };
        Self {
            path: self.path.clone(),
            line: end,
            side,
            start_line: (start != end).then_some(start),
            start_side: (start != end).then_some(side),
        }
    }

    /// The first line the anchor covers: `start_line` for a range, `line`
    /// otherwise.
    pub fn anchor_start(&self) -> u32 {
        self.start_line.unwrap_or(self.line)
    }

    /// Whether `line` on `side` falls inside this anchor.
    pub fn covers(&self, path: &str, line: u32, side: Side) -> bool {
        self.path == path && self.side == side && (self.anchor_start()..=self.line).contains(&line)
    }
}

/// Whether drafts written against `drafted_against` can no longer be trusted
/// now that the pull request's head is `current`.
///
/// A force-push moves the head commit, invalidating every drafted line
/// anchor. An unknown sha on either side is treated as "not stale": blocking
/// review submission because a field could not be read would be worse than
/// the risk it guards against.
pub fn drafts_are_stale(drafted_against: Option<&str>, current: &str) -> bool {
    match drafted_against {
        Some(drafted) => !drafted.is_empty() && !current.is_empty() && drafted != current,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extending_an_anchor_orders_the_range() {
        let anchor = DraftAnchor::single("src/main.rs", 20, Side::Right);

        let downwards = anchor.extended_to(24, Side::Right);
        assert_eq!((downwards.start_line, downwards.line), (Some(20), 24));

        // Extending *above* the original line still yields start <= line.
        let upwards = anchor.extended_to(16, Side::Right);
        assert_eq!((upwards.start_line, upwards.line), (Some(16), 20));
    }

    #[test]
    fn extending_onto_the_same_line_stays_single_line() {
        let anchor =
            DraftAnchor::single("src/main.rs", 20, Side::Right).extended_to(20, Side::Right);
        assert_eq!(anchor.start_line, None);
        assert_eq!(anchor.start_side, None);
        assert_eq!(anchor.line, 20);
    }

    #[test]
    fn a_range_carries_its_side_on_both_ends() {
        let anchor = DraftAnchor::single("a.rs", 3, Side::Left).extended_to(5, Side::Left);
        assert_eq!(anchor.start_side, Some(Side::Left));
        assert_eq!(anchor.side, Side::Left);
        assert_eq!(anchor.anchor_start(), 3);
    }

    #[test]
    fn covers_reports_every_line_in_the_range() {
        let anchor =
            DraftAnchor::single("src/main.rs", 20, Side::Right).extended_to(24, Side::Right);
        for line in 20..=24 {
            assert!(
                anchor.covers("src/main.rs", line, Side::Right),
                "line {line}"
            );
        }
        assert!(!anchor.covers("src/main.rs", 19, Side::Right));
        assert!(!anchor.covers("src/main.rs", 25, Side::Right));
        // Same numbers on the other side are a different anchor entirely.
        assert!(!anchor.covers("src/main.rs", 22, Side::Left));
        assert!(!anchor.covers("other.rs", 22, Side::Right));
    }

    #[test]
    fn drafts_written_against_the_current_head_are_fresh() {
        assert!(!drafts_are_stale(Some("abc"), "abc"));
    }

    #[test]
    fn drafts_written_against_an_older_head_are_stale() {
        assert!(drafts_are_stale(Some("abc"), "def"));
    }

    #[test]
    fn no_drafts_is_never_stale() {
        assert!(!drafts_are_stale(None, "abc"));
    }

    /// An unreadable sha must not block the user from submitting.
    #[test]
    fn unknown_shas_do_not_block_submission() {
        assert!(!drafts_are_stale(Some(""), "abc"));
        assert!(!drafts_are_stale(Some("abc"), ""));
    }
}
