//! The editing state behind [`super::TextInput`], free of gpui.
//!
//! Every offset this type receives is untrusted: it may come from the
//! platform IME (UTF-16 code units, possibly reversed or past the end), from
//! hit-testing a layout shaped for different text, or from a mouse position.
//! Each public operation clamps such offsets into the content and onto a
//! `char` boundary before using them, and normalises reversed ranges, so the
//! invariant below holds after any sequence of calls and slicing the content
//! can never panic.
//!
//! **Invariant.** `selected` and `marked` are `start <= end` ranges whose ends
//! lie on `char` boundaries of `text` (and therefore within it).
//!
//! Caret movement steps over whole grapheme clusters, but the invariant is
//! deliberately only `char` boundaries: an IME may legitimately place the
//! caret between a base character and a combining mark it is composing.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// Whether an operation changed the text, so the view knows to emit
/// [`super::InputEvent::Changed`].
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Changed,
    Unchanged,
}

impl Edit {
    pub fn changed(self) -> bool {
        self == Edit::Changed
    }
}

/// A broken buffer invariant. Only ever produced by a bug in this module; the
/// buffer checks itself after every mutation in debug builds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvariantViolation {
    Reversed {
        which: &'static str,
        range: Range<usize>,
    },
    OutOfBounds {
        which: &'static str,
        range: Range<usize>,
        len: usize,
    },
    NotCharBoundary {
        which: &'static str,
        offset: usize,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TextBuffer {
    text: String,
    /// Byte offsets into `text`.
    selected: Range<usize>,
    /// The caret is at `selected.start` rather than `selected.end`.
    reversed: bool,
    /// IME pre-edit region, byte offsets into `text`.
    marked: Option<Range<usize>>,
    /// Bumped on every change to `text`, so geometry shaped for an earlier
    /// version can be recognised as stale.
    revision: u64,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    // --- reading -----------------------------------------------------------

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn selected_range(&self) -> Range<usize> {
        self.selected.clone()
    }

    pub fn is_reversed(&self) -> bool {
        self.reversed
    }

    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The end of the selection the caret sits at.
    pub fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selected.clone()]
    }

    // --- sanitising --------------------------------------------------------

    /// `offset` clamped into the text and moved back onto a `char` boundary.
    pub fn clamp_offset(&self, offset: usize) -> usize {
        floor_char_boundary(&self.text, offset)
    }

    /// `range` with both ends clamped and the ends put in order.
    pub fn clamp_range(&self, range: &Range<usize>) -> Range<usize> {
        let (a, b) = (self.clamp_offset(range.start), self.clamp_offset(range.end));
        a.min(b)..a.max(b)
    }

    // --- whole-text --------------------------------------------------------

    /// Replace the whole text, leaving the caret at the end.
    pub fn set_text(&mut self, text: impl Into<String>) -> Edit {
        self.text = text.into();
        let end = self.text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        self.bump();
        Edit::Changed
    }

    // --- editing -----------------------------------------------------------

    /// Replace `range` (byte offsets, untrusted) with `new_text`, leaving the
    /// caret after the insertion and ending any composition.
    pub fn replace_range(&mut self, range: Range<usize>, new_text: &str) -> Edit {
        let range = self.clamp_range(&range);
        let edit = self.splice(range.clone(), new_text);
        let caret = range.start + new_text.len();
        self.selected = caret..caret;
        self.reversed = false;
        self.marked = None;
        self.check();
        edit
    }

    /// Type or paste over the selection.
    pub fn insert(&mut self, new_text: &str) -> Edit {
        self.replace_range(self.selected.clone(), new_text)
    }

    pub fn backspace(&mut self) -> Edit {
        let range = if self.selected.is_empty() {
            self.previous_boundary(self.selected.start)..self.selected.start
        } else {
            self.selected.clone()
        };
        self.replace_range(range, "")
    }

    pub fn delete(&mut self) -> Edit {
        let range = if self.selected.is_empty() {
            self.selected.start..self.next_boundary(self.selected.start)
        } else {
            self.selected.clone()
        };
        self.replace_range(range, "")
    }

    /// Remove the selection, returning what it held; `None` if it was empty.
    pub fn cut(&mut self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        let text = self.selected_text().to_owned();
        let _ = self.replace_range(self.selected.clone(), "");
        Some(text)
    }

    // --- caret movement ----------------------------------------------------

    /// Collapse the selection to `offset` (untrusted).
    pub fn move_to(&mut self, offset: usize) {
        let offset = self.clamp_offset(offset);
        self.selected = offset..offset;
        self.reversed = false;
        self.check();
    }

    /// Move the caret end of the selection to `offset` (untrusted), keeping
    /// the other end as the anchor.
    pub fn select_to(&mut self, offset: usize) {
        let offset = self.clamp_offset(offset);
        let anchor = if self.reversed {
            self.selected.end
        } else {
            self.selected.start
        };
        if offset < anchor {
            self.selected = offset..anchor;
            self.reversed = true;
        } else {
            self.selected = anchor..offset;
            self.reversed = false;
        }
        self.check();
    }

    pub fn select_all(&mut self) {
        self.selected = 0..self.text.len();
        self.reversed = false;
        self.check();
    }

    pub fn move_left(&mut self) {
        let target = if self.selected.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected.start
        };
        self.move_to(target);
    }

    pub fn move_right(&mut self) {
        let target = if self.selected.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected.end
        };
        self.move_to(target);
    }

    pub fn select_left(&mut self) {
        self.select_to(self.previous_boundary(self.cursor()));
    }

    pub fn select_right(&mut self) {
        self.select_to(self.next_boundary(self.cursor()));
    }

    /// Start of the caret's logical line.
    pub fn home(&mut self) {
        let cursor = self.cursor();
        let start = self.text[..cursor].rfind('\n').map_or(0, |ix| ix + 1);
        self.move_to(start);
    }

    /// End of the caret's logical line, before a `\r\n` as well as a `\n`, so
    /// the caret never lands inside the CRLF grapheme.
    pub fn end(&mut self) {
        let cursor = self.cursor();
        let end = match self.text[cursor..].find('\n') {
            Some(ix) => {
                let newline = cursor + ix;
                if newline > cursor && self.text.as_bytes()[newline - 1] == b'\r' {
                    newline - 1
                } else {
                    newline
                }
            }
            None => self.text.len(),
        };
        self.move_to(end);
    }

    /// Start of the grapheme before `offset`, or 0.
    pub fn previous_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(ix, _)| (ix < offset).then_some(ix))
            .unwrap_or(0)
    }

    /// Start of the grapheme after `offset`, or the end of the text.
    pub fn next_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(ix, _)| (ix > offset).then_some(ix))
            .unwrap_or(self.text.len())
    }

    // --- UTF-16, for the platform IME ---------------------------------------

    /// Byte offset of the UTF-16 offset `target`. Past the end clamps to the
    /// end; inside a surrogate pair rounds up to after the character.
    pub fn offset_from_utf16(&self, target: usize) -> usize {
        utf8_offset_in(&self.text, target)
    }

    /// UTF-16 offset of the byte offset `target`. Past the end clamps to the
    /// end; inside a character rounds up to after it.
    pub fn offset_to_utf16(&self, target: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.text.chars() {
            if utf8 >= target {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    /// A UTF-16 range as an ordered byte range on `char` boundaries.
    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let (a, b) = (
            self.offset_from_utf16(range.start),
            self.offset_from_utf16(range.end),
        );
        a.min(b)..a.max(b)
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    /// The text in a UTF-16 range, with the UTF-16 range actually returned.
    pub fn text_for_utf16_range(&self, range_utf16: &Range<usize>) -> (String, Range<usize>) {
        let range = self.range_from_utf16(range_utf16);
        let actual = self.range_to_utf16(&range);
        (self.text[range].to_owned(), actual)
    }

    pub fn selected_range_utf16(&self) -> Range<usize> {
        self.range_to_utf16(&self.selected)
    }

    pub fn marked_range_utf16(&self) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    /// The range an IME edit targets: the given UTF-16 range, else the
    /// composition, else the selection.
    fn ime_target(&self, range_utf16: Option<&Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selected.clone())
    }

    /// Commit `new_text` from the IME (or plain typing, which gpui routes
    /// through the same call).
    pub fn ime_replace(&mut self, range_utf16: Option<Range<usize>>, new_text: &str) -> Edit {
        let range = self.ime_target(range_utf16.as_ref());
        self.replace_range(range, new_text)
    }

    /// Replace the target with an in-progress composition and mark it.
    ///
    /// `new_selected_utf16` is relative to `new_text`, not to the whole text —
    /// it is where the IME wants the caret inside its pre-edit string.
    pub fn ime_replace_and_mark(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_utf16: Option<Range<usize>>,
    ) -> Edit {
        let range = self.ime_target(range_utf16.as_ref());
        let edit = self.splice(range.clone(), new_text);

        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected = match new_selected_utf16 {
            Some(selected) => {
                let (a, b) = (
                    utf8_offset_in(new_text, selected.start),
                    utf8_offset_in(new_text, selected.end),
                );
                range.start + a.min(b)..range.start + a.max(b)
            }
            None => {
                let caret = range.start + new_text.len();
                caret..caret
            }
        };
        self.reversed = false;
        self.check();
        edit
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    // --- internals ---------------------------------------------------------

    /// Replace an already-sanitised range. Callers fix up the selection.
    fn splice(&mut self, range: Range<usize>, new_text: &str) -> Edit {
        if range.is_empty() && new_text.is_empty() {
            return Edit::Unchanged;
        }
        self.text.replace_range(range, new_text);
        self.bump();
        Edit::Changed
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn check(&self) {
        debug_assert_eq!(self.invariant_violation(), None);
    }

    /// The first way the buffer breaks its invariant, if any.
    pub fn invariant_violation(&self) -> Option<InvariantViolation> {
        let ranges = std::iter::once(("selected", &self.selected))
            .chain(self.marked.as_ref().map(|marked| ("marked", marked)));
        for (which, range) in ranges {
            if range.start > range.end {
                return Some(InvariantViolation::Reversed {
                    which,
                    range: range.clone(),
                });
            }
            if range.end > self.text.len() {
                return Some(InvariantViolation::OutOfBounds {
                    which,
                    range: range.clone(),
                    len: self.text.len(),
                });
            }
            for offset in [range.start, range.end] {
                if !self.text.is_char_boundary(offset) {
                    return Some(InvariantViolation::NotCharBoundary { which, offset });
                }
            }
        }
        None
    }
}

/// `offset` clamped to `text.len()` and moved back to a `char` boundary.
pub fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Byte offset in `text` of the UTF-16 offset `target`: clamped to the end,
/// rounded up past a character whose surrogate pair it splits.
fn utf8_offset_in(text: &str, target: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf16 >= target {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

#[cfg(test)]
mod fuzz;
#[cfg(test)]
mod tests;
