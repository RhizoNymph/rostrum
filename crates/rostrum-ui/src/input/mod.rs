//! A multi-line text input.
//!
//! GPUI ships no text input widget — the platform gives you an IME protocol
//! (`EntityInputHandler`) and text shaping, and the rest is yours. This is a
//! trimmed-down multi-line version of gpui's `examples/input.rs`, sufficient
//! for writing review comments.
//!
//! The editing state lives in the gpui-free [`TextBuffer`], which sanitises
//! every offset it is handed; [`TextInput`] is the view around it — actions,
//! mouse handling, and the IME protocol — and [`element`] shapes and paints it.

mod buffer;
mod element;
mod geometry;

use std::ops::Range;

use gpui::{
    App, Bounds, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, UTF16Selection,
    Window, actions, div, point, prelude::*, px,
};

pub use buffer::{Edit, InvariantViolation, TextBuffer};
use element::TextElement;
use geometry::{LayoutSource, TextLayoutCache};

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
    buffer: TextBuffer,
    placeholder: SharedString,
    min_lines: usize,
    max_lines: usize,
    /// Shaped state from the last paint, used for hit-testing. May describe
    /// older text or the placeholder; see [`Self::current_layout`].
    layout: Option<TextLayoutCache>,
    bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            buffer: TextBuffer::new(),
            placeholder: placeholder.into(),
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
        self.buffer.text()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.text().trim().is_empty()
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text: SharedString = text.into();
        let edit = self.buffer.set_text(text.as_str());
        self.after_edit(edit, cx);
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    /// Repaint, and tell the owner when the text changed.
    fn after_edit(&mut self, edit: Edit, cx: &mut Context<Self>) {
        if edit.changed() {
            cx.emit(InputEvent::Changed);
        }
        cx.notify();
    }

    // --- editing -----------------------------------------------------------

    fn backspace(&mut self, _: &Backspace, _window: &mut Window, cx: &mut Context<Self>) {
        let edit = self.buffer.backspace();
        self.after_edit(edit, cx);
    }

    fn delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        let edit = self.buffer.delete();
        self.after_edit(edit, cx);
    }

    fn newline(&mut self, _: &Newline, _window: &mut Window, cx: &mut Context<Self>) {
        let edit = self.buffer.insert("\n");
        self.after_edit(edit, cx);
    }

    fn submit(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submit);
    }

    // --- cursor movement ---------------------------------------------------

    fn left(&mut self, _: &Left, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left();
        cx.notify();
    }

    fn right(&mut self, _: &Right, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right();
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_left();
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_right();
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.notify();
    }

    fn up(&mut self, _: &Up, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_in_adjacent_row(-1) {
            self.buffer.move_to(offset);
            cx.notify();
        }
    }

    fn down(&mut self, _: &Down, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_in_adjacent_row(1) {
            self.buffer.move_to(offset);
            cx.notify();
        }
    }

    fn home(&mut self, _: &Home, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.home();
        cx.notify();
    }

    fn end(&mut self, _: &End, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.end();
        cx.notify();
    }

    /// Move the caret one visual row up or down, preserving x where possible.
    /// `None` until a layout of the current text has been painted.
    fn offset_in_adjacent_row(&self, delta: i32) -> Option<usize> {
        let layout = self.current_layout()?;
        let position = layout.position_for_offset(self.buffer.cursor())?;
        let target = point(position.x, position.y + layout.line_height * delta as f32);
        layout.offset_for_position(target)
    }

    // --- clipboard ---------------------------------------------------------

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.buffer.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_owned()));
        }
    }

    fn cut(&mut self, _: &Cut, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.buffer.cut() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.after_edit(Edit::Changed, cx);
        }
    }

    fn paste(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let edit = self.buffer.insert(&text);
        self.after_edit(edit, cx);
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
                self.buffer.select_to(offset);
            } else {
                self.buffer.move_to(offset);
            }
            cx.notify();
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A release outside the input never reaches `on_mouse_up`, so the
        // button state is what ends a drag.
        if event.pressed_button != Some(MouseButton::Left) {
            self.is_selecting = false;
        }
        if self.is_selecting
            && let Some(offset) = self.offset_for_window_position(event.position)
        {
            self.buffer.select_to(offset);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    // --- geometry ----------------------------------------------------------

    /// The cached layout, if it was shaped from the text as it is now.
    fn current_layout(&self) -> Option<&TextLayoutCache> {
        self.layout
            .as_ref()
            .filter(|layout| layout.is_current(self.buffer.revision()))
    }

    /// Byte offset under a window-space point. The result is a hint: the
    /// buffer clamps it again when it is used.
    fn offset_for_window_position(&self, position: Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        if layout.source == LayoutSource::Placeholder && self.buffer.is_empty() {
            // The placeholder's glyphs are not text; the only offset is 0.
            return Some(0);
        }
        let bounds = self.bounds?;
        self.current_layout()?
            .offset_for_position(position - bounds.origin)
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
        let (text, actual) = self.buffer.text_for_utf16_range(&range_utf16);
        actual_range.replace(actual);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.selected_range_utf16(),
            reversed: self.buffer.is_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.buffer.marked_range_utf16()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.unmark();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let edit = self.buffer.ime_replace(range_utf16, new_text);
        self.after_edit(edit, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let edit =
            self.buffer
                .ime_replace_and_mark(range_utf16, new_text, new_selected_range_utf16);
        self.after_edit(edit, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.current_layout()?;
        let range = self.buffer.range_from_utf16(&range_utf16);
        let start = layout.position_for_offset(range.start)?;
        let end = layout.position_for_offset(range.end)?;
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
        Some(
            self.buffer
                .offset_to_utf16(self.buffer.clamp_offset(offset)),
        )
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

/// Key bindings for text widgets. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding;

    // Read-only selection binds `ctrl-c`/`cmd-c` with no key context, so it has
    // to be registered alongside the input bindings that shadow them — and
    // before them, so a focused input wins the tie. This is also the one startup
    // hook the application already calls.
    crate::selection::init(cx);

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
