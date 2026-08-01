//! A multi-line text input.
//!
//! GPUI ships no text input widget — the platform gives you an IME protocol
//! (`EntityInputHandler`) and text shaping, and the rest is yours. This is a
//! trimmed-down multi-line version of gpui's `examples/input.rs`, sufficient
//! for writing review comments.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::ActiveTheme;

actions!(
    input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        SelectLeft,
        SelectRight,
        SelectAll,
        Newline,
        Submit,
        Copy,
        Cut,
        Paste,
    ]
);

/// Emitted so the owning view can react without polling.
#[derive(Clone, Debug)]
pub enum InputEvent {
    Changed,
    Submit,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    /// Byte offsets into `content`.
    selected_range: Range<usize>,
    selection_reversed: bool,
    /// IME pre-edit region.
    marked_range: Option<Range<usize>>,
    min_lines: usize,
    max_lines: usize,
    /// Shaped state from the last paint, used for hit-testing.
    layout: Option<TextLayoutCache>,
    bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

struct TextLayoutCache {
    lines: Vec<WrappedLine>,
    /// Byte offset in `content` at which each shaped line begins.
    starts: Vec<usize>,
    line_height: Pixels,
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            min_lines: 3,
            max_lines: 14,
            layout: None,
            bounds: None,
            is_selecting: false,
        }
    }

    pub fn lines(mut self, min: usize, max: usize) -> Self {
        self.min_lines = min.max(1);
        self.max_lines = max.max(self.min_lines);
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    // --- editing -----------------------------------------------------------

    fn replace(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        let mut next = String::with_capacity(self.content.len() + text.len());
        next.push_str(&self.content[..range.start]);
        next.push_str(text);
        next.push_str(&self.content[range.end..]);
        self.content = next.into();

        let caret = range.start + text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _window: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            self.previous_boundary(self.selected_range.start)..self.selected_range.start
        } else {
            self.selected_range.clone()
        };
        self.replace(range, "", cx);
    }

    fn delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            self.selected_range.start..self.next_boundary(self.selected_range.start)
        } else {
            self.selected_range.clone()
        };
        self.replace(range, "", cx);
    }

    fn newline(&mut self, _: &Newline, _window: &mut Window, cx: &mut Context<Self>) {
        let range = self.selected_range.clone();
        self.replace(range, "\n", cx);
    }

    fn submit(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submit);
    }

    // --- cursor movement ---------------------------------------------------

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn left(&mut self, _: &Left, _window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected_range.start
        };
        self.move_to(target, cx);
    }

    fn right(&mut self, _: &Right, _window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected_range.end
        };
        self.move_to(target, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _window: &mut Window, cx: &mut Context<Self>) {
        let target = self.previous_boundary(self.cursor());
        self.select_to(target, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _window: &mut Window, cx: &mut Context<Self>) {
        let target = self.next_boundary(self.cursor());
        self.select_to(target, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn up(&mut self, _: &Up, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_in_adjacent_row(-1) {
            self.move_to(offset, cx);
        }
    }

    fn down(&mut self, _: &Down, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_in_adjacent_row(1) {
            self.move_to(offset, cx);
        }
    }

    fn home(&mut self, _: &Home, _window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let start = self.content[..cursor].rfind('\n').map_or(0, |ix| ix + 1);
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &End, _window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor();
        let end = self.content[cursor..]
            .find('\n')
            .map_or(self.content.len(), |ix| cursor + ix);
        self.move_to(end, cx);
    }

    /// Move the caret one visual row up or down, preserving x where possible.
    fn offset_in_adjacent_row(&self, delta: i32) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let position = self.position_for_offset(layout, self.cursor())?;
        let target = point(position.x, position.y + layout.line_height * delta as f32);
        self.offset_for_position(layout, target)
    }

    // --- clipboard ---------------------------------------------------------

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            let text = self.content[self.selected_range.clone()].to_string();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &Cut, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            let text = self.content[self.selected_range.clone()].to_string();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            let range = self.selected_range.clone();
            self.replace(range, "", cx);
        }
    }

    fn paste(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let range = self.selected_range.clone();
        self.replace(range, &text, cx);
    }

    // --- mouse -------------------------------------------------------------

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        if let Some(offset) = self.offset_for_window_position(event.position) {
            if event.modifiers.shift {
                self.select_to(offset, cx);
            } else {
                self.move_to(offset, cx);
            }
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting
            && let Some(offset) = self.offset_for_window_position(event.position)
        {
            self.select_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    // --- geometry ----------------------------------------------------------

    fn offset_for_window_position(&self, position: Point<Pixels>) -> Option<usize> {
        let bounds = self.bounds?;
        let layout = self.layout.as_ref()?;
        self.offset_for_position(layout, position - bounds.origin)
    }

    /// Caret position relative to the input's origin.
    fn position_for_offset(
        &self,
        layout: &TextLayoutCache,
        offset: usize,
    ) -> Option<Point<Pixels>> {
        let mut y = px(0.);
        for (line, start) in layout.lines.iter().zip(&layout.starts) {
            let end = start + line.len();
            if offset <= end {
                let local = line.position_for_index(offset - start, layout.line_height)?;
                return Some(point(local.x, y + local.y));
            }
            y += line.size(layout.line_height).height;
        }
        None
    }

    fn offset_for_position(
        &self,
        layout: &TextLayoutCache,
        position: Point<Pixels>,
    ) -> Option<usize> {
        let mut y = px(0.);
        for (line, start) in layout.lines.iter().zip(&layout.starts) {
            let height = line.size(layout.line_height).height;
            if position.y < y + height || std::ptr::eq(line, layout.lines.last()?) {
                let local = point(position.x, (position.y - y).max(px(0.)));
                let index = line
                    .index_for_position(local, layout.line_height)
                    .unwrap_or_else(|closest| closest);
                return Some(start + index.min(line.len()));
            }
            y += height;
        }
        Some(self.content.len())
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(ix, _)| (ix < offset).then_some(ix))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(ix, _)| (ix > offset).then_some(ix))
            .unwrap_or(self.content.len())
    }

    fn offset_from_utf16(&self, target: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= target {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, target: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= target {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }
}

impl EventEmitter<InputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content.get(range)?.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        let mut next = String::with_capacity(self.content.len() + new_text.len());
        next.push_str(&self.content[..range.start]);
        next.push_str(new_text);
        next.push_str(&self.content[range.end..]);
        self.content = next.into();

        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| range.start + r.start..range.start + r.end)
            .unwrap_or_else(|| {
                let caret = range.start + new_text.len();
                caret..caret
            });
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = self.position_for_offset(layout, range.start)?;
        let end = self.position_for_offset(layout, range.end)?;
        Some(Bounds::from_corners(
            bounds.origin + start,
            bounds.origin + point(end.x, end.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_for_window_position(point)?;
        Some(self.offset_to_utf16(offset))
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .cursor(gpui::CursorStyle::IBeam)
            .w_full()
            .px_2()
            .py_1p5()
            .bg(theme.background)
            .border_1()
            .border_color(if self.focus_handle.is_focused(_window) {
                theme.accent
            } else {
                theme.border
            })
            .rounded_tl(px(6.))
            .rounded_tr(px(6.))
            .rounded_bl(px(6.))
            .rounded_br(px(6.))
            .child(TextElement {
                input: cx.entity().clone(),
            })
    }
}

/// Custom element: shapes the text, paints selection, caret, and glyphs, and
/// installs the platform IME handler.
struct TextElement {
    input: Entity<TextInput>,
}

struct Prepaint {
    lines: Vec<WrappedLine>,
    starts: Vec<usize>,
    quads: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    line_height: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();

        let input = self.input.clone();
        let line_height = window.line_height();
        let layout_id =
            window.request_measured_layout(style, move |known, _available, window, cx| {
                let width = known.width.unwrap_or(px(400.));
                let (min, max) = {
                    let input = input.read(cx);
                    (input.min_lines, input.max_lines)
                };
                let rows = shape(&input, width, window, cx)
                    .map(|(lines, _)| {
                        lines
                            .iter()
                            .map(|line| line.wrap_boundaries().len() + 1)
                            .sum::<usize>()
                    })
                    .unwrap_or(min);
                size(width, line_height * rows.clamp(min, max) as f32)
            });

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let line_height = window.line_height();
        let Some((lines, starts)) = shape(&self.input, bounds.size.width, window, cx) else {
            return Prepaint {
                lines: Vec::new(),
                starts: Vec::new(),
                quads: Vec::new(),
                caret: None,
                line_height,
            };
        };

        let input = self.input.read(cx);
        let theme = cx.theme();
        let cache = TextLayoutCache {
            lines: Vec::new(),
            starts: starts.clone(),
            line_height,
        };
        // Positions are computed against the freshly shaped lines rather than
        // the cached ones, which are only updated at paint time.
        let locate = |offset: usize| -> Option<Point<Pixels>> {
            let mut y = px(0.);
            for (line, start) in lines.iter().zip(&cache.starts) {
                if offset <= start + line.len() {
                    let local = line.position_for_index(offset - start, line_height)?;
                    return Some(point(local.x, y + local.y));
                }
                y += line.size(line_height).height;
            }
            None
        };

        let mut quads = Vec::new();
        if !input.selected_range.is_empty()
            && let (Some(start), Some(end)) = (
                locate(input.selected_range.start),
                locate(input.selected_range.end),
            )
        {
            let color = gpui::Hsla {
                a: 0.30,
                ..theme.accent
            };
            if (start.y - end.y).abs() < px(0.5) {
                quads.push(fill(
                    Bounds::from_corners(
                        bounds.origin + start,
                        bounds.origin + point(end.x, end.y + line_height),
                    ),
                    color,
                ));
            } else {
                // First row runs to the right edge, whole rows between are
                // full width, and the last row starts at the left edge.
                quads.push(fill(
                    Bounds::from_corners(
                        bounds.origin + start,
                        bounds.origin + point(bounds.size.width, start.y + line_height),
                    ),
                    color,
                ));
                let mut y = start.y + line_height;
                while y < end.y - px(0.5) {
                    quads.push(fill(
                        Bounds::from_corners(
                            bounds.origin + point(px(0.), y),
                            bounds.origin + point(bounds.size.width, y + line_height),
                        ),
                        color,
                    ));
                    y += line_height;
                }
                quads.push(fill(
                    Bounds::from_corners(
                        bounds.origin + point(px(0.), end.y),
                        bounds.origin + point(end.x, end.y + line_height),
                    ),
                    color,
                ));
            }
        }

        let caret = input
            .selected_range
            .is_empty()
            .then(|| locate(input.cursor()))
            .flatten()
            .map(|position| {
                fill(
                    Bounds::new(bounds.origin + position, size(px(1.5), line_height)),
                    theme.accent,
                )
            });

        Prepaint {
            lines,
            starts,
            quads,
            caret,
            line_height,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        for quad in prepaint.quads.drain(..) {
            window.paint_quad(quad);
        }

        let mut origin = bounds.origin;
        for line in &prepaint.lines {
            if let Err(error) = line.paint(
                origin,
                prepaint.line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            ) {
                tracing::warn!(%error, "failed to paint input line");
            }
            origin.y += line.size(prepaint.line_height).height;
        }

        if focus_handle.is_focused(window)
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }

        let lines = std::mem::take(&mut prepaint.lines);
        let starts = std::mem::take(&mut prepaint.starts);
        let line_height = prepaint.line_height;
        self.input.update(cx, |input, _cx| {
            input.layout = Some(TextLayoutCache {
                lines,
                starts,
                line_height,
            });
            input.bounds = Some(bounds);
        });
    }
}

/// Shape the input's content, returning the wrapped lines and the byte offset
/// at which each begins.
fn shape(
    input: &Entity<TextInput>,
    width: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Option<(Vec<WrappedLine>, Vec<usize>)> {
    let input = input.read(cx);
    let style = window.text_style();
    let showing_placeholder = input.content.is_empty();
    let display: SharedString = if showing_placeholder {
        input.placeholder.clone()
    } else {
        input.content.clone()
    };

    let color = if showing_placeholder {
        cx.theme().text_subtle
    } else {
        style.color
    };

    let base = TextRun {
        len: display.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    // Underline the IME pre-edit region so composition is visible.
    let runs = match input.marked_range.as_ref().filter(|_| !showing_placeholder) {
        Some(marked) => vec![
            TextRun {
                len: marked.start,
                ..base.clone()
            },
            TextRun {
                len: marked.end - marked.start,
                underline: Some(UnderlineStyle {
                    color: Some(color),
                    thickness: px(1.),
                    wavy: false,
                }),
                ..base.clone()
            },
            TextRun {
                len: display.len() - marked.end,
                ..base
            },
        ]
        .into_iter()
        .filter(|run| run.len > 0)
        .collect(),
        None => vec![base],
    };

    let font_size = style.font_size.to_pixels(window.rem_size());
    let lines = window
        .text_system()
        .shape_text(display.clone(), font_size, &runs, Some(width), None)
        .ok()?;

    // `shape_text` splits on newlines; reconstruct each line's byte offset.
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        starts.push(offset);
        offset += line.len() + 1; // + the newline that was consumed
    }

    Some((lines.into_iter().collect(), starts))
}

/// Key bindings for inputs. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("up", Up, Some("TextInput")),
        KeyBinding::new("down", Down, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("enter", Newline, Some("TextInput")),
        KeyBinding::new("ctrl-enter", Submit, Some("TextInput")),
        KeyBinding::new("cmd-enter", Submit, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
    ]);
}
