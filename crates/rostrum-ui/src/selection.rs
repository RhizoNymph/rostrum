//! Read-only text selection.
//!
//! GPUI ships no drag-selection primitive for text you cannot edit:
//! `InteractiveText` gives click and hover, and `TextInput` gives selection
//! only because it owns an `EntityInputHandler`. [`SelectableText`] fills the
//! gap — a block of shaped text that can be swept with the mouse and copied.
//!
//! The geometry is the same as [`crate::input`]: shape via `StyledText`, map
//! byte offsets to pixels with `TextLayout::position_for_index`, and paint one
//! quad per visual row behind the glyphs.
//!
//! **Scope limit, deliberate.** A selection lives inside a single text block.
//! Dragging out of one paragraph and into the next does not extend the
//! selection across them; the pointer just keeps sweeping the block it started
//! in. Only one block in the window may hold a selection at a time.

use std::{ops::Range, rc::Rc};

use gpui::{
    App, Bounds, ClipboardItem, CursorStyle, DispatchPhase, Element, ElementId, Global,
    GlobalElementId, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, SharedString, StyledText, TextLayout, TextRun, Window, actions, fill, point, px,
};

use crate::theme::ActiveTheme;

actions!(selection, [CopySelection]);

/// How far the mouse may travel between press and release and still count as a
/// click rather than a drag. A drag never fires a click handler, so links stay
/// clickable while sweeping over one only selects it.
const DRAG_THRESHOLD: Pixels = px(3.);

/// Opacity of the selection highlight painted behind the glyphs.
const SELECTION_ALPHA: f32 = 0.30;

// --- global state ----------------------------------------------------------

/// The window's one and only text selection, plus any drag in flight.
///
/// Selection state deliberately lives here rather than in per-element state
/// keyed by `GlobalElementId`. Two reasons: only one block may be selected at
/// a time, which a single global makes unrepresentable to violate — starting a
/// selection anywhere overwrites the previous one with no cross-element
/// bookkeeping; and the `ctrl-c` handler is a global action listener with no
/// element in scope, so it needs somewhere global to read the selection from.
/// A `Global` also survives re-render for free, which is what per-element
/// state would have bought us.
#[derive(Default)]
struct SelectionState {
    current: Option<Selection>,
    drag: Option<Drag>,
    /// Guards [`init`] so it is idempotent.
    initialized: bool,
}

impl Global for SelectionState {}

struct Selection {
    id: ElementId,
    /// A copy of the block's text, so the copy action can resolve `range`
    /// without an element in scope.
    text: SharedString,
    range: Range<usize>,
}

impl Selection {
    fn selected_text(&self) -> Option<&str> {
        self.text.get(self.range.clone()).filter(|s| !s.is_empty())
    }
}

struct Drag {
    id: ElementId,
    /// Selection the press established: empty for a single click, the word for
    /// a double click, the whole block for a triple click. Dragging grows the
    /// selection outwards from this range.
    anchor: Range<usize>,
    down_index: usize,
    origin: Point<Pixels>,
    moved: bool,
}

/// Register the copy binding and its handler. Idempotent; call once at startup.
///
/// Must be called *before* any binding it should lose to. These bindings carry
/// no key context so they apply anywhere, which gpui scores at the full depth
/// of the context stack — the same depth a focused `TextInput` scores its own
/// `ctrl-c` at. Ties break towards the binding registered later, so registering
/// this first is what leaves an editable input's copy in charge while it has
/// focus. [`crate::input::bind_keys`] calls it in that order.
pub fn init(cx: &mut App) {
    if std::mem::replace(&mut cx.default_global::<SelectionState>().initialized, true) {
        return;
    }

    cx.bind_keys([
        KeyBinding::new("ctrl-c", CopySelection, None),
        KeyBinding::new("cmd-c", CopySelection, None),
    ]);

    cx.on_action(|_: &CopySelection, cx: &mut App| {
        let Some(text) = cx
            .default_global::<SelectionState>()
            .current
            .as_ref()
            .and_then(Selection::selected_text)
            .map(str::to_owned)
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    });
}

/// Mirror the selection into the X11/Wayland primary selection buffer, which
/// middle-click paste reads. Convention on Linux is to do this on mouse-up.
fn write_primary_selection(cx: &mut App) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let Some(text) = cx
            .default_global::<SelectionState>()
            .current
            .as_ref()
            .and_then(Selection::selected_text)
            .map(str::to_owned)
        else {
            return;
        };
        cx.write_to_primary(ClipboardItem::new_string(text));
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = cx;
    }
}

// --- element ---------------------------------------------------------------

/// Called with the index of the clicked range from [`SelectableText::on_click_ranges`].
type ClickHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A read-only block of text that supports mouse selection and copy.
///
/// See the [module docs](self) for the single-block scope limit.
pub struct SelectableText {
    id: ElementId,
    text: SharedString,
    styled: StyledText,
    click_ranges: Vec<Range<usize>>,
    click_handler: Option<ClickHandler>,
}

impl SelectableText {
    /// `id` must be unique in the window; selection state is keyed by it.
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        Self {
            id: id.into(),
            text: text.clone(),
            styled: StyledText::new(text),
            click_ranges: Vec::new(),
            click_handler: None,
        }
    }

    /// Per-byte-range styling, same contract as gpui `TextRun`s: runs must tile
    /// the string exactly, no gaps or overlaps.
    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        self.styled = self.styled.with_runs(runs);
        self
    }

    /// Per-byte-range styling layered over the inherited text style. Unlike
    /// [`Self::with_runs`], ranges may leave gaps — uncovered bytes keep the
    /// style they inherit — and the font family and size come from the
    /// surrounding element rather than being baked in by the caller.
    pub fn with_highlights(
        mut self,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        self.styled = self.styled.with_highlights(highlights);
        self
    }

    /// Clickable ranges, for links inside rendered markdown. The handler
    /// receives the index of the range that was clicked. A press that moved
    /// far enough to count as a drag never fires it.
    pub fn on_click_ranges(
        mut self,
        ranges: Vec<Range<usize>>,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click_ranges = ranges;
        self.click_handler = Some(Rc::new(handler));
        self
    }

    fn cursor_style(&self, layout: &TextLayout, window: &Window) -> CursorStyle {
        let hovering_link = layout
            .index_for_position(window.mouse_position())
            .is_ok_and(|index| self.click_ranges.iter().any(|range| range.contains(&index)));
        if hovering_link {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        }
    }

    fn paint_selection(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(range) = cx
            .default_global::<SelectionState>()
            .current
            .as_ref()
            .filter(|selection| selection.id == self.id)
            .map(|selection| selection.range.clone())
            .filter(|range| !range.is_empty())
        else {
            return;
        };

        let layout = self.styled.layout();
        let (Some(start), Some(end)) = (
            layout.position_for_index(range.start),
            layout.position_for_index(range.end),
        ) else {
            return;
        };

        let color = Hsla {
            a: SELECTION_ALPHA,
            ..cx.theme().accent
        };
        let line_height = window.pixel_snap(window.line_height());
        for quad in selection_quads(bounds, start, end, line_height, color) {
            window.paint_quad(quad);
        }
    }

    fn on_mouse_down(&self, layout: TextLayout, hitbox: Hitbox, window: &mut Window) {
        let id = self.id.clone();
        let text = self.text.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !hitbox.is_hovered(window)
            {
                return;
            }
            let index = index_for_position(&layout, &text, event.position);
            let anchor = match event.click_count {
                0 | 1 => index..index,
                2 => word_range_at(&text, index),
                _ => 0..text.len(),
            };

            let state = cx.default_global::<SelectionState>();
            state.current = Some(Selection {
                id: id.clone(),
                text: text.clone(),
                range: anchor.clone(),
            });
            state.drag = Some(Drag {
                id: id.clone(),
                anchor,
                down_index: index,
                origin: event.position,
                moved: false,
            });
            window.refresh();
        });
    }

    fn on_mouse_move(&self, layout: TextLayout, window: &mut Window) {
        let id = self.id.clone();
        let text = self.text.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            if !cx
                .default_global::<SelectionState>()
                .drag
                .as_ref()
                .is_some_and(|drag| drag.id == id)
            {
                return;
            }

            // Deliberately not gated on the hitbox: dragging past the edge of
            // the block keeps extending the selection to the nearest offset.
            let index = index_for_position(&layout, &text, event.position);
            let state = cx.default_global::<SelectionState>();
            let Some(drag) = state.drag.as_mut() else {
                return;
            };
            if exceeds_drag_threshold(drag.origin, event.position) {
                drag.moved = true;
            }
            let range = drag.anchor.start.min(index)..drag.anchor.end.max(index);
            state.current = Some(Selection {
                id: id.clone(),
                text: text.clone(),
                range,
            });
            window.refresh();
        });
    }

    fn on_mouse_up(&self, layout: TextLayout, window: &mut Window) {
        let id = self.id.clone();
        let text = self.text.clone();
        let click_ranges = self.click_ranges.clone();
        let click_handler = self.click_handler.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let state = cx.default_global::<SelectionState>();
            if !state.drag.as_ref().is_some_and(|drag| drag.id == id) {
                return;
            }
            let Some(drag) = state.drag.take() else {
                return;
            };

            let empty = state
                .current
                .as_ref()
                .is_none_or(|selection| selection.range.is_empty());

            if drag.moved || !empty {
                write_primary_selection(cx);
                window.refresh();
                return;
            }

            // A press that neither moved nor selected anything is a click.
            if let Some(handler) = click_handler.as_ref() {
                let up_index = index_for_position(&layout, &text, event.position);
                let clicked = click_ranges.iter().position(|range| {
                    range.contains(&drag.down_index) && range.contains(&up_index)
                });
                if let Some(index) = clicked {
                    handler(index, window, cx);
                }
            }
            window.refresh();
        });
    }
}

impl IntoElement for SelectableText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<gpui::accesskit::Role> {
        Some(gpui::accesskit::Role::Label)
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        node.set_value(self.text.to_string());
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.styled.request_layout(None, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled
            .prepaint(None, inspector_id, bounds, request_layout, window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Behind the glyphs.
        self.paint_selection(bounds, window, cx);
        self.styled.paint(
            None,
            inspector_id,
            bounds,
            request_layout,
            &mut (),
            window,
            cx,
        );

        let layout = self.styled.layout().clone();
        window.set_cursor_style(self.cursor_style(&layout, window), hitbox);
        self.on_mouse_down(layout.clone(), hitbox.clone(), window);
        self.on_mouse_move(layout.clone(), window);
        self.on_mouse_up(layout, window);
    }
}

// --- pure helpers ----------------------------------------------------------

/// Nearest byte offset to `position`, clamped into `text`.
///
/// Needs a laid-out `TextLayout`, so it is not unit tested; it is kept to the
/// one job of turning gpui's `Result<usize, usize>` — exact hit versus nearest
/// miss — into an offset we can always use.
fn index_for_position(layout: &TextLayout, text: &str, position: Point<Pixels>) -> usize {
    let index = match layout.index_for_position(position) {
        Ok(index) | Err(index) => index,
    };
    clamp_to_char_boundary(text, index)
}

/// One quad per visual row of the selection: the first row runs to the right
/// edge, whole rows between are full width, and the last row starts at the
/// left edge. `start` and `end` are window-space caret positions.
fn selection_quads(
    bounds: Bounds<Pixels>,
    start: Point<Pixels>,
    end: Point<Pixels>,
    line_height: Pixels,
    color: Hsla,
) -> Vec<PaintQuad> {
    // Same row: one quad from caret to caret.
    if (start.y - end.y).abs() < px(0.5) {
        return vec![fill(
            Bounds::from_corners(start, point(end.x, end.y + line_height)),
            color,
        )];
    }

    let mut quads = vec![fill(
        Bounds::from_corners(start, point(bounds.right(), start.y + line_height)),
        color,
    )];
    let mut y = start.y + line_height;
    while y < end.y - px(0.5) {
        quads.push(fill(
            Bounds::from_corners(
                point(bounds.left(), y),
                point(bounds.right(), y + line_height),
            ),
            color,
        ));
        y += line_height;
    }
    quads.push(fill(
        Bounds::from_corners(
            point(bounds.left(), end.y),
            point(end.x, end.y + line_height),
        ),
        color,
    ));
    quads
}

/// Whether the pointer travelled far enough between press and release to count
/// as a drag rather than a click.
fn exceeds_drag_threshold(origin: Point<Pixels>, position: Point<Pixels>) -> bool {
    (position.x - origin.x).abs() > DRAG_THRESHOLD || (position.y - origin.y).abs() > DRAG_THRESHOLD
}

fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// What a character counts as when growing a double-click selection.
///
/// Deliberately character classes rather than UAX #29 word bounds: UAX #29
/// keeps `foo.bar` and `1,000` together as one word, which is right for prose
/// but wrong for a code review tool where double-clicking `foo` in `foo.bar`
/// should give you `foo`. `char::is_alphanumeric` is still Unicode-aware, so
/// non-ASCII scripts work.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Whitespace,
    Other,
}

fn class_of(ch: char) -> CharClass {
    if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else if ch.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Other
    }
}

/// Byte range of the word a double click at `offset` should select.
///
/// The offset arrives from hit-testing, so it sits between two characters
/// rather than on one; both sides are considered. A word on either side wins
/// over whitespace or punctuation, preferring the character after the offset;
/// failing that, the run of whitespace or punctuation the offset lands in is
/// selected. Offsets are clamped into the string and onto a character
/// boundary, so a mid-codepoint offset can never slice a multi-byte character.
fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = clamp_to_char_boundary(text, offset);
    let after = text[offset..].chars().next();
    let before = text[..offset].chars().next_back();

    // `seed` is the byte offset of the character whose class we expand over.
    let (seed, class) = match (after, before) {
        (Some(ch), _) if class_of(ch) == CharClass::Word => (offset, CharClass::Word),
        (_, Some(ch)) if class_of(ch) == CharClass::Word => {
            (offset - ch.len_utf8(), CharClass::Word)
        }
        (Some(ch), _) => (offset, class_of(ch)),
        (None, Some(ch)) => (offset - ch.len_utf8(), class_of(ch)),
        (None, None) => return offset..offset,
    };

    let mut start = seed;
    for (ix, ch) in text[..seed].char_indices().rev() {
        if class_of(ch) != class {
            break;
        }
        start = ix;
    }

    let mut end = seed;
    for (ix, ch) in text[seed..].char_indices() {
        if class_of(ch) != class {
            break;
        }
        end = seed + ix + ch.len_utf8();
    }

    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- word boundaries ---------------------------------------------------

    #[test]
    fn selects_the_word_under_the_offset() {
        assert_eq!(word_range_at("hello world", 2), 0..5);
        assert_eq!(word_range_at("hello world", 8), 6..11);
    }

    #[test]
    fn selects_the_first_word_at_the_start_of_the_string() {
        assert_eq!(word_range_at("hello world", 0), 0..5);
    }

    #[test]
    fn selects_the_last_word_at_the_end_of_the_string() {
        let text = "hello world";
        assert_eq!(word_range_at(text, text.len()), 6..11);
    }

    #[test]
    fn prefers_the_word_before_the_offset_over_following_whitespace() {
        // Offset 5 sits between "hello" and the space.
        assert_eq!(word_range_at("hello world", 5), 0..5);
    }

    #[test]
    fn prefers_the_word_after_the_offset_over_preceding_whitespace() {
        assert_eq!(word_range_at("hello world", 6), 6..11);
    }

    #[test]
    fn splits_on_punctuation() {
        assert_eq!(word_range_at("foo.bar", 1), 0..3);
        assert_eq!(word_range_at("foo.bar", 4), 4..7);
        // Between "foo" and ".": the word wins over the punctuation.
        assert_eq!(word_range_at("foo.bar", 3), 0..3);
    }

    #[test]
    fn selects_a_punctuation_run_when_no_word_touches_the_offset() {
        assert_eq!(word_range_at("a --- b", 3), 2..5);
    }

    #[test]
    fn selects_a_whitespace_run_when_no_word_touches_the_offset() {
        // Offset 2 sits between the two spaces of "a  b".
        assert_eq!(word_range_at("a  b", 2), 1..3);
    }

    #[test]
    fn does_not_run_a_word_selection_past_punctuation() {
        assert_eq!(word_range_at("(paren)", 3), 1..6);
    }

    #[test]
    fn treats_underscores_as_part_of_a_word() {
        assert_eq!(word_range_at("some_ident here", 4), 0..10);
    }

    #[test]
    fn handles_an_empty_string() {
        assert_eq!(word_range_at("", 0), 0..0);
        assert_eq!(word_range_at("", 7), 0..0);
    }

    #[test]
    fn handles_a_single_word() {
        assert_eq!(word_range_at("word", 0), 0..4);
        assert_eq!(word_range_at("word", 2), 0..4);
        assert_eq!(word_range_at("word", 4), 0..4);
    }

    #[test]
    fn handles_multi_byte_characters() {
        // "héllo wörld": é and ö are two bytes each.
        let text = "héllo wörld";
        assert_eq!(text.len(), 13);
        assert_eq!(word_range_at(text, 3), 0..6);
        assert_eq!(word_range_at(text, 9), 7..13);
        assert_eq!(word_range_at(text, text.len()), 7..13);
    }

    #[test]
    fn clamps_an_offset_inside_a_multi_byte_character() {
        // Offset 2 is the second byte of "é", not a char boundary.
        let text = "héllo";
        assert_eq!(word_range_at(text, 2), 0..6);
    }

    #[test]
    fn clamps_an_offset_past_the_end_of_the_string() {
        assert_eq!(word_range_at("hello", 900), 0..5);
    }

    #[test]
    fn handles_an_emoji_grapheme_cluster() {
        let text = "ok 👍 go";
        let thumb = text.find('👍').unwrap_or_default();
        let range = word_range_at(text, thumb);
        assert_eq!(&text[range], "👍");
    }

    #[test]
    fn handles_scripts_without_spaces() {
        let text = "日本語";
        let range = word_range_at(text, 0);
        assert!(!range.is_empty());
        assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
    }

    // --- drag threshold ----------------------------------------------------

    #[test]
    fn a_stationary_press_is_not_a_drag() {
        let origin = point(px(10.), px(10.));
        assert!(!exceeds_drag_threshold(origin, origin));
    }

    #[test]
    fn a_jittery_press_is_not_a_drag() {
        let origin = point(px(10.), px(10.));
        assert!(!exceeds_drag_threshold(origin, point(px(12.), px(11.))));
    }

    #[test]
    fn a_press_exactly_at_the_threshold_is_not_a_drag() {
        let origin = point(px(10.), px(10.));
        assert!(!exceeds_drag_threshold(
            origin,
            point(px(10.) + DRAG_THRESHOLD, px(10.))
        ));
    }

    #[test]
    fn a_horizontal_sweep_is_a_drag() {
        let origin = point(px(10.), px(10.));
        assert!(exceeds_drag_threshold(origin, point(px(40.), px(10.))));
    }

    #[test]
    fn a_vertical_sweep_is_a_drag() {
        let origin = point(px(10.), px(10.));
        assert!(exceeds_drag_threshold(origin, point(px(10.), px(40.))));
    }

    #[test]
    fn a_backwards_sweep_is_a_drag() {
        let origin = point(px(40.), px(40.));
        assert!(exceeds_drag_threshold(origin, point(px(10.), px(38.))));
    }

    // --- selection geometry ------------------------------------------------

    fn block() -> Bounds<Pixels> {
        Bounds::from_corners(point(px(0.), px(0.)), point(px(100.), px(60.)))
    }

    #[test]
    fn a_selection_within_one_row_is_a_single_quad() {
        let quads = selection_quads(
            block(),
            point(px(10.), px(0.)),
            point(px(40.), px(0.)),
            px(20.),
            gpui::hsla(0., 1., 0.5, 1.),
        );
        assert_eq!(quads.len(), 1);
        assert_eq!(
            quads[0].bounds,
            Bounds::from_corners(point(px(10.), px(0.)), point(px(40.), px(20.)))
        );
    }

    #[test]
    fn a_selection_across_two_rows_is_two_quads() {
        let quads = selection_quads(
            block(),
            point(px(60.), px(0.)),
            point(px(30.), px(20.)),
            px(20.),
            gpui::hsla(0., 1., 0.5, 1.),
        );
        assert_eq!(quads.len(), 2);
        // First row runs to the right edge.
        assert_eq!(quads[0].bounds.right(), px(100.));
        // Last row starts at the left edge.
        assert_eq!(quads[1].bounds.left(), px(0.));
        assert_eq!(quads[1].bounds.right(), px(30.));
    }

    #[test]
    fn whole_rows_between_the_ends_are_full_width() {
        let quads = selection_quads(
            block(),
            point(px(60.), px(0.)),
            point(px(30.), px(60.)),
            px(20.),
            gpui::hsla(0., 1., 0.5, 1.),
        );
        assert_eq!(quads.len(), 4);
        for quad in &quads[1..3] {
            assert_eq!(quad.bounds.left(), px(0.));
            assert_eq!(quad.bounds.right(), px(100.));
        }
    }
}
