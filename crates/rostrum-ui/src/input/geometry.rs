//! Shaped-text geometry for [`super::TextInput`]: mapping between byte offsets
//! and pixel positions, and the record of which text a layout was shaped for.
//!
//! A layout is cached at paint time and used for hit-testing until the next
//! paint. Between the two the text can change — a keystroke followed by Up
//! before a frame is drawn — and an empty input is shaped from its
//! placeholder rather than its text. [`LayoutSource`] records which, so the
//! view only hit-tests against a layout of the text it currently holds.
//! Offsets read from a layout are still passed through the buffer's clamping.

use std::ops::Range;

use gpui::{Pixels, Point, WrappedLine, point, px};

/// What a layout was shaped from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LayoutSource {
    /// The input's text at this buffer revision.
    Content { revision: u64 },
    /// The placeholder, shown because the text was empty. Its offsets mean
    /// nothing in the (empty) text.
    Placeholder,
}

/// Shaped state from the last paint, used for hit-testing.
pub(super) struct TextLayoutCache {
    pub lines: Vec<WrappedLine>,
    /// Byte offset at which each shaped line begins.
    pub starts: Vec<usize>,
    pub line_height: Pixels,
    pub source: LayoutSource,
}

impl TextLayoutCache {
    /// Whether this layout describes the text at `revision`.
    pub fn is_current(&self, revision: u64) -> bool {
        self.source == LayoutSource::Content { revision }
    }

    pub fn position_for_offset(&self, offset: usize) -> Option<Point<Pixels>> {
        position_for_offset(&self.lines, &self.starts, self.line_height, offset)
    }

    /// Nearest byte offset to `position`, relative to the input's origin.
    pub fn offset_for_position(&self, position: Point<Pixels>) -> Option<usize> {
        let last = self.lines.len().checked_sub(1)?;
        let mut y = px(0.);
        for (ix, (line, start)) in self.lines.iter().zip(&self.starts).enumerate() {
            let height = line.size(self.line_height).height;
            if position.y < y + height || ix == last {
                let local = point(position.x, (position.y - y).max(px(0.)));
                let index = line
                    .index_for_position(local, self.line_height)
                    .unwrap_or_else(|closest| closest);
                return Some(start + index.min(line.len()));
            }
            y += height;
        }
        None
    }
}

/// Caret position for `offset`, relative to the input's origin.
pub(super) fn position_for_offset(
    lines: &[WrappedLine],
    starts: &[usize],
    line_height: Pixels,
    offset: usize,
) -> Option<Point<Pixels>> {
    let mut y = px(0.);
    for (line, &start) in lines.iter().zip(starts) {
        if offset <= start + line.len() {
            let local = line.position_for_index(offset.checked_sub(start)?, line_height)?;
            return Some(point(local.x, y + local.y));
        }
        y += line.size(line_height).height;
    }
    None
}

/// Byte offset at which each line of `text` begins, as `shape_text` splits
/// it: on `\n` only, consuming the newline.
pub(super) fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(ix, _)| ix + 1))
        .collect()
}

/// Lengths of the text runs that tile `len` bytes, the second of the three
/// underlined for an IME composition. Zero-length runs are dropped; a marked
/// range outside `0..len` is clamped rather than trusted.
pub(super) fn run_lengths(len: usize, marked: Option<Range<usize>>) -> Vec<(usize, bool)> {
    let Some(marked) = marked else {
        return vec![(len, false)];
    };
    let end = marked.end.min(len);
    let start = marked.start.min(end);
    [(start, false), (end - start, true), (len - end, false)]
        .into_iter()
        .filter(|(run, _)| *run > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_starts_follow_newlines_only() {
        assert_eq!(line_starts(""), vec![0]);
        assert_eq!(line_starts("ab"), vec![0]);
        assert_eq!(line_starts("a\nb—\n"), vec![0, 2, 7]);
        assert_eq!(line_starts("a\r\nb"), vec![0, 3]);
    }

    #[test]
    fn runs_tile_the_text() {
        assert_eq!(run_lengths(5, None), vec![(5, false)]);
        assert_eq!(
            run_lengths(5, Some(1..3)),
            vec![(1, false), (2, true), (2, false)]
        );
        assert_eq!(run_lengths(5, Some(0..5)), vec![(5, true)]);
        assert_eq!(run_lengths(0, None), vec![(0, false)]);
    }

    #[test]
    fn runs_never_trust_a_marked_range_outside_the_text() {
        // The pre-fix code computed `len - marked.end`, which underflows.
        for marked in [3..9, 9..12, Range { start: 4, end: 2 }] {
            let runs = run_lengths(5, Some(marked));
            assert_eq!(runs.iter().map(|(run, _)| run).sum::<usize>(), 5);
        }
    }
}
