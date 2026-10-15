//! The grid's horizontal scroll, kept by the view rather than by an
//! `overflow_x_scroll` container.
//!
//! The row-header column stays put while the check columns slide under it,
//! and the one header row slides with them, so both are drawn shifted by the
//! same offset. Keeping the offset here also lets each row draw only the
//! columns in view ([`HScroll::visible`]): with one column set across every
//! repository, a row can hold far more columns than fit on screen.
//!
//! Pure arithmetic on pixel widths, so it is tested without a window.

use std::ops::Range;

/// Columns drawn beyond each edge of the viewport, so a partly visible
/// column at either side is never missing.
const OVERSCAN: usize = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HScroll {
    offset: f32,
}

impl HScroll {
    /// How far the columns are scrolled left, in pixels.
    pub(crate) fn offset(&self) -> f32 {
        self.offset
    }

    fn max(content: f32, viewport: f32) -> f32 {
        (content - viewport).max(0.)
    }

    /// Keep the offset within `0..=content - viewport`, as after a resize or
    /// a rebuild that dropped columns.
    pub(crate) fn clamp(&mut self, content: f32, viewport: f32) {
        self.offset = self.offset.clamp(0., Self::max(content, viewport));
    }

    /// Apply a wheel or trackpad delta: positive `delta` moves the content
    /// right (towards the start), as GPUI's scroll deltas do.
    pub(crate) fn scroll_by(&mut self, delta: f32, content: f32, viewport: f32) {
        self.offset -= delta;
        self.clamp(content, viewport);
    }

    /// Scroll the least that shows `start..start + width` whole; when it is
    /// wider than the viewport, its start.
    pub(crate) fn reveal(&mut self, start: f32, width: f32, viewport: f32) {
        let end = start + width;
        if start < self.offset || width >= viewport {
            self.offset = start;
        } else if end > self.offset + viewport {
            self.offset = end - viewport;
        }
        self.offset = self.offset.max(0.);
    }

    /// The columns of `cell` width, of `count`, that a `viewport` wide view
    /// shows, with [`OVERSCAN`] either side.
    pub(crate) fn visible(&self, count: usize, cell: f32, viewport: f32) -> Range<usize> {
        if count == 0 || cell <= 0. {
            return 0..0;
        }
        let first = (self.offset / cell).floor() as usize;
        let last = ((self.offset + viewport.max(0.)) / cell).ceil() as usize;
        first.saturating_sub(OVERSCAN).min(count)..(last + OVERSCAN).min(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: f32 = 100.;

    fn at(offset: f32) -> HScroll {
        HScroll { offset }
    }

    #[test]
    fn scrolling_stays_within_the_content() {
        let mut scroll = HScroll::default();
        scroll.scroll_by(50., 1000., 300.);
        assert_eq!(scroll.offset(), 0., "nothing before the first column");
        scroll.scroll_by(-450., 1000., 300.);
        assert_eq!(scroll.offset(), 450.);
        scroll.scroll_by(-1000., 1000., 300.);
        assert_eq!(scroll.offset(), 700., "the last column ends at the edge");
    }

    #[test]
    fn content_narrower_than_the_view_never_scrolls() {
        let mut scroll = at(120.);
        scroll.clamp(200., 300.);
        assert_eq!(scroll.offset(), 0.);
        scroll.scroll_by(-80., 200., 300.);
        assert_eq!(scroll.offset(), 0.);
    }

    #[test]
    fn a_shrunk_grid_pulls_the_offset_back() {
        let mut scroll = at(700.);
        scroll.clamp(600., 300.);
        assert_eq!(scroll.offset(), 300.);
    }

    #[test]
    fn revealing_scrolls_the_least_that_shows_the_column() {
        // Already in view: nothing moves.
        let mut scroll = at(100.);
        scroll.reveal(200., CELL, 300.);
        assert_eq!(scroll.offset(), 100.);
        // Off the right edge: its end meets the edge.
        scroll.reveal(500., CELL, 300.);
        assert_eq!(scroll.offset(), 300.);
        // Off the left edge: its start meets the edge.
        scroll.reveal(100., CELL, 300.);
        assert_eq!(scroll.offset(), 100.);
        // Wider than the view: its start.
        scroll.reveal(400., 500., 300.);
        assert_eq!(scroll.offset(), 400.);
    }

    #[test]
    fn only_columns_in_view_are_drawn_with_one_either_side() {
        assert_eq!(at(0.).visible(20, CELL, 300.), 0..4);
        assert_eq!(at(450.).visible(20, CELL, 300.), 3..9);
        assert_eq!(at(1700.).visible(20, CELL, 300.), 16..20);
        assert_eq!(at(0.).visible(2, CELL, 300.), 0..2);
        assert_eq!(at(0.).visible(0, CELL, 300.), 0..0);
        assert_eq!(at(0.).visible(5, CELL, 0.), 0..1);
    }
}
