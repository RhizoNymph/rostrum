# Feature: ui_foundation

The local UI layer built directly on `gpui`: theme, components, text primitives,
and markdown rendering.

## Scope

- Application bootstrap, window creation, fonts and assets.
- Theme definition and access.
- Reusable components: button, icon button, label, chip/badge, avatar, card
  chrome, tooltip, spinner, tab bar, scrollbar, context menu.
- Markdown parsing and rendering.
- Byte-range text selection and clipboard.
- Actions and keybindings.

## Non-scope

- Feature-specific views — those live in `crates/rostrum/src/{feed,detail}`.
- Syntax highlighting — see `diff_review`.

## Why not Zed's `ui` and `theme` crates

They are technically usable standalone — `ui` depends only on `theme`, and
`theme` needs just `theme::init(LoadThemes::JustBase, cx)` with no `settings`
crate involved — and they would save considerable component work.

They are **GPL-3.0-or-later**. `gpui` and `gpui_platform` are Apache-2.0.
Depending on `ui`, `theme`, or `syntax_theme` would make rostrum GPL. That is the
deciding factor; the components are built locally.

The same caution applies to copying Zed's `highlights.scm` query files or
vendoring `syntax_theme`. Grammar queries should come from upstream grammar repos
under their own (usually MIT) terms.

## Dependencies

```toml
[workspace.dependencies]
gpui          = { git = "https://github.com/zed-industries/zed", rev = "<pinned>" }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "<pinned>",
                  features = ["wayland", "x11"] }
gpui_tokio    = { git = "https://github.com/zed-industries/zed", rev = "<pinned>" }
```

`gpui` is published to crates.io as 0.2.2, but **do not mix sources**.
`gpui_platform` is unpublished and declares `gpui = { path = "crates/gpui" }`
with no version field, so a git dependency on it resolves `gpui` from the git
checkout. Adding `gpui = "0.2.2"` from crates.io alongside it compiles two
distinct `gpui` crates, and the resulting trait-mismatch errors are extremely
confusing. Pin all three to the same rev.

Platform features: `wayland` and `x11` on Linux (GPUI picks at runtime),
`font-kit` on macOS, none on Windows.

## Bootstrap

```rust
use gpui::{App, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

fn main() {
    application().run(|cx: &mut App| {
        gpui_tokio::init(cx);
        theme::init(cx);
        load_fonts(cx);
        bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)),
                            ..Default::default() },
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        ).expect("failed to open window");
        cx.activate(true);
    });
}
```

Fonts are embedded via an `AssetSource` implementation and registered with
`cx.text_system().add_fonts(..)`. A UI sans face and a monospace face for diffs
are bundled rather than relying on system font availability.

## Theme

A plain struct stored as a GPUI global, with light and dark variants selected by
config or system appearance:

```rust
pub struct Theme {
    pub colors: Colors,     // bg, surface, border, text, text_muted, accent, ...
    pub status: Status,     // success, warning, error, info
    pub diff: DiffColors,   // added/removed/context bg+fg, word-level emphasis
    pub syntax: SyntaxTheme,// capture name -> HighlightStyle
    pub sizes: Sizes,       // spacing scale, radii, row heights
    pub fonts: Fonts,
}
```

`SyntaxTheme` maps tree-sitter capture names to `HighlightStyle` with
longest-dotted-prefix matching (`string.escape` falls back to `string`) — a small
`BTreeMap` and a prefix walk, not worth a dependency.

Accessed through an extension trait: `cx.theme()`.

## Components

Built as `RenderOnce` + `#[derive(IntoElement)]` — GPUI's stateless component
pattern. Stateful widgets (composer, context menu) are `Entity<T>` with `Render`.

`Popover` (`crates/rostrum-ui/src/popover.rs`) is a floating panel anchored to a
button: the panel's top-left corner sits on the centre of its trigger. It is
placed by layout rather than by measuring: a zero-size box at `left/top: 50%` of
a `relative` wrapper holds `deferred(anchored())`. It floats over the content
below instead of pushing it down. A press outside both the panel and the trigger
calls `on_dismiss`. The view owns a `PopoverAnchor`, into which the trigger's
bounds are recorded every frame, so pressing the trigger of an open popover is
left to the trigger's own click. Otherwise the popover would close on mouse-down
and reopen on the click. Binding `escape` is up to the owner.

Two things bite repeatedly and are worth stating once:

- **`.id()` is required before `.on_click()` or `.overflow_y_scroll()`.** Those
  methods live on `StatefulInteractiveElement`, which only exists for elements
  given a stable `ElementId`. GPUI needs that identity to persist click and
  scroll state across frames.
- **`cx.notify()` is never automatic.** Every state mutation that should repaint
  must call it explicitly.

Layout helpers `h_flex()`/`v_flex()` are trivial to define locally and make the
rest of the code read like Zed's.

## Markdown

Zed's `markdown` crate is excellent but depends on `language`, `settings`,
`theme_settings`, and `ui` — heavy, and GPL. We render markdown ourselves:

`pulldown-cmark` → an intermediate `MarkdownBlock` tree → GPUI elements. PR
bodies and comments need headings, paragraphs, emphasis, inline and fenced code,
lists, blockquotes, links, images, task lists, and tables. Fenced code blocks
route through the same tree-sitter highlighting path as diffs.

Parsing happens once when a comment is received, not in `render`; the parsed
model is cached on the timeline item.

GitHub-specific extensions (`@mentions`, `#123` issue references, permalinks,
`:emoji:`) are handled as a post-parse pass that rewrites text spans into links.

## Text and selection

`StyledText::new(text).with_runs(runs)` renders one string with per-byte-range
styling. `TextRun` carries a UTF-8 `len` and no start offset — runs concatenate
and must cover the string exactly, no gaps or overlaps.
`StyledText::with_highlights` is the alternative that fills gaps automatically
from the ambient text style, requiring sorted, non-overlapping, char-boundary
ranges.

GPUI has **no built-in drag-selection for read-only text**. `InteractiveText`
supports click, hover, and tooltips only.

`crates/rostrum-ui/src/input/` implements the *editable* text input — a
byte-range selection, mouse drag handling, `position_for_index` /
`index_for_position` hit-testing against wrapped lines, a painted selection quad
per visual row, IME composition, and clipboard integration.

## Text input

Split by concern:

| File | Role | Key items |
|---|---|---|
| `input/buffer.rs` | gpui-free editing state | `TextBuffer`, `Edit`, `InvariantViolation`, `floor_char_boundary` |
| `input/geometry.rs` | Offset ⇄ pixel mapping over a cached layout | `TextLayoutCache`, `LayoutSource`, `line_starts`, `run_lengths` |
| `input/element.rs` | Shaping, painting, IME handler install | `TextElement`, `shape` |
| `input/mod.rs` | The `TextInput` view: actions, mouse, `EntityInputHandler`, `bind_keys` | `TextInput`, `InputEvent` |
| `input/buffer/tests.rs`, `input/buffer/fuzz.rs` | Regression tests and a deterministic fuzzer (xorshift, fixed seeds) | — |

Control flow: every action, mouse event and IME call on `TextInput` turns into
one `TextBuffer` call, then `cx.notify()` and — when the call returns
`Edit::Changed` — `InputEvent::Changed`. `TextElement` reads the buffer to shape
the text (or the placeholder when the text is empty), paints selection and
caret from the freshly shaped lines, and stores the layout on the view at paint
time tagged with a `LayoutSource`: `Content { revision }` or `Placeholder`.

**Offsets are untrusted.** They arrive as UTF-16 ranges from the platform IME
(possibly reversed or past the end), from a layout shaped for older text or for
the placeholder, and from mouse positions. `TextBuffer` therefore clamps every
incoming offset into the text and back onto a `char` boundary and orders every
range before using it. The IME's `new_selected_range` is relative to the
pre-edit string and is converted within it, not within the whole text.

**Stale geometry.** The buffer bumps `revision` on every text change. The view
hit-tests (Up/Down, mouse, `bounds_for_range`) only against a layout whose
source is the current revision; a placeholder layout maps any click to offset 0.
Anything that slips through is clamped by the buffer anyway.

Caret movement (left/right, backspace/delete) steps over whole grapheme
clusters; End stops before a `\r\n`. The invariant is deliberately only `char`
boundaries, because an IME may put the caret between a base character and a
combining mark it is composing.

`crates/rostrum-ui/src/selection.rs` adds `SelectableText` for read-only text:
click-drag to select, double-click for a word, triple-click for the block, and
copy to the clipboard (plus the primary selection on Linux). It wraps gpui's
`StyledText` and reuses its `TextLayout` for hit-testing, so no shaping code is
duplicated. The markdown renderer emits it, which makes comments, descriptions,
and review bodies selectable.

Selection state is a single `Global` rather than per-element state. That makes
"only one block is selected at a time" impossible to violate — starting a
selection overwrites the previous one with no cross-element bookkeeping — and
gives the copy action, which runs with no element in scope, something to read
from. The `Selection` carries a copy of the block's text so the range can be
resolved without reaching back into the element.

Two subtleties worth preserving:

- **Binding order.** GPUI scores a context-less binding at the full depth of the
  context stack, so the selection's `ctrl-c` *ties* with a focused `TextInput`'s
  rather than losing to it, and ties break toward whichever was registered
  later. `selection::init` must therefore run before the input bindings.
- **Click versus drag.** A registered click range fires only if the press
  neither exceeded a 3px threshold nor produced a non-empty selection. Get this
  wrong and either links stop working or every selection navigates away.

Word selection uses character classes (alphanumeric/underscore, whitespace,
other) rather than UAX #29. UAX #29 keeps `foo.bar` and `1,000` together, which
is right for prose and wrong for a review tool, where double-clicking `foo`
should yield `foo`.

Selection does not span separate blocks; dragging out of a paragraph does not
extend into the next. The diff uses whole-line selection instead — see
`docs/features/diff_review.md`.

## Actions and keybindings

Declared with the `actions!` macro and bound with `cx.bind_keys([KeyBinding::new(..)])`.
Element handlers use `.on_action(cx.listener(Self::method))`.

Focus uses `FocusHandle` stored on entities, `Focusable` implemented by cloning
it out, and `.track_focus(&handle)` on the root element of `render`. Key contexts
(`.key_context("Feed")`, `"PrDetail"`, `"Composer"`) scope bindings so `j`/`k`
navigate the feed without hijacking typing in the composer.

## Invariants

- Highlight runs exactly cover the string they style.
- Every interactive or scrollable element has a stable `.id()`.
- Every state mutation intended to repaint calls `cx.notify()`.
- Subscriptions are retained in a field or detached; dropping unsubscribes.
- Long-lived `Task`s are held in a field; dropping cancels them.
- Markdown is parsed off the render path.
- `TextBuffer`: `selected` and `marked` are ordered ranges within the text whose
  ends are `char` boundaries, after any sequence of public calls; checked by
  `debug_assert` after each mutation and by the fuzzer after each step.
- `TextBuffer::revision` changes whenever the text changes; the input never
  hit-tests against a layout of a different revision or of the placeholder.
- Text runs built for shaping tile the displayed string exactly
  (`geometry::run_lengths` clamps the marked range).

## Files

| File | Role |
|---|---|
| `crates/rostrum/src/main.rs` | Bootstrap, window, fonts, keybindings |
| `crates/rostrum/src/workspace.rs` | Root view, split layout |
| `crates/rostrum-ui/src/theme/mod.rs` | `Theme`, `cx.theme()`, light/dark |
| `crates/rostrum-ui/src/components/` | Button, label, chip, avatar, card, tabs, ... |
| `crates/rostrum-ui/src/input/` | Editable text input; see *Text input* above |
| `crates/rostrum-ui/src/selection.rs` | Byte-range selection, hit-testing, clipboard |
| `crates/rostrum-ui/src/markdown.rs` | `MarkdownBlock` → elements |
| `crates/rostrum-md/src/lib.rs` | `pulldown-cmark` → `MarkdownBlock`, GitHub extensions |
| `crates/rostrum-ui/src/assets.rs` | Embedded fonts and icons |
