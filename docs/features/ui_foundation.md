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
supports click, hover, and tooltips only. Selection is hand-rolled following Zed's
`markdown` crate: a byte-range `Selection` struct, manual mouse handlers,
hit-testing through `TextLayout::index_for_position`, a manually painted
selection quad per visible row, and `cx.write_to_clipboard` (plus
`cx.write_to_primary` on Linux for middle-click paste).

This lives in `crates/rostrum-ui/src/selection.rs` and is shared by the markdown
renderer and the diff view.

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

## Files

| File | Role |
|---|---|
| `crates/rostrum/src/main.rs` | Bootstrap, window, fonts, keybindings |
| `crates/rostrum/src/workspace.rs` | Root view, split layout |
| `crates/rostrum-ui/src/theme/mod.rs` | `Theme`, `cx.theme()`, light/dark |
| `crates/rostrum-ui/src/components/` | Button, label, chip, avatar, card, tabs, ... |
| `crates/rostrum-ui/src/selection.rs` | Byte-range selection, hit-testing, clipboard |
| `crates/rostrum-ui/src/markdown.rs` | `MarkdownBlock` → elements |
| `crates/rostrum-md/src/lib.rs` | `pulldown-cmark` → `MarkdownBlock`, GitHub extensions |
| `crates/rostrum-ui/src/assets.rs` | Embedded fonts and icons |
