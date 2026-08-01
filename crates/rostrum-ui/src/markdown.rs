//! Renders a [`rostrum_md::Document`] as GPUI elements.
//!
//! Zed's `markdown` crate would do this, but it depends on `language`,
//! `settings`, `theme_settings`, and `ui` — all GPL-3.0-or-later. This walks
//! the tree directly instead.

use std::ops::Range;

use gpui::{
    AnyElement, App, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString,
    StrikethroughStyle, StyledText, UnderlineStyle, div, prelude::*, px, rems,
};
use rostrum_md::{Block, Document, Inline, ListItem};

use crate::{components::v_flex, theme::Theme};

/// Renders markdown blocks into a vertical stack.
///
/// `id_seed` must be unique among the markdown bodies rendered in one window —
/// use the index of the comment being rendered. Every interactive text run
/// derives its element id from it, and duplicate ids would make GPUI attribute
/// one comment's clicks to another.
pub fn render_document(document: &Document, id_seed: usize, theme: &Theme, cx: &App) -> AnyElement {
    let mut renderer = Renderer {
        name: SharedString::from(format!("md-{id_seed}")),
        counter: 0,
        theme,
    };
    v_flex()
        .gap_2()
        .children(
            document
                .blocks
                .iter()
                .map(|block| renderer.block(block, cx)),
        )
        .into_any_element()
}

/// Parse and render in one step.
pub fn render_source(source: &str, id_seed: usize, theme: &Theme, cx: &App) -> AnyElement {
    render_document(&rostrum_md::parse(source), id_seed, theme, cx)
}

/// Parse with GitHub shorthand expansion (`@user`, `#123`) and render.
pub fn render_github(
    source: &str,
    owner: &str,
    repo: &str,
    id_seed: usize,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let context = rostrum_md::GitHubContext::new(owner, repo);
    render_document(
        &rostrum_md::parse_github(source, &context),
        id_seed,
        theme,
        cx,
    )
}

struct Renderer<'a> {
    name: SharedString,
    counter: usize,
    theme: &'a Theme,
}

impl Renderer<'_> {
    fn next_id(&mut self) -> gpui::ElementId {
        self.counter += 1;
        gpui::ElementId::NamedInteger(self.name.clone(), self.counter as u64)
    }

    fn block(&mut self, block: &Block, cx: &App) -> AnyElement {
        match block {
            Block::Paragraph(inlines) => self.inlines(inlines, self.base_style(), cx),
            Block::Heading { level, children } => {
                let size = match level {
                    1 => 1.35,
                    2 => 1.2,
                    3 => 1.08,
                    _ => 0.95,
                };
                div()
                    .mt_1()
                    .text_size(rems(size))
                    .text_color(self.theme.text)
                    .child(self.inlines(children, self.base_style(), cx))
                    .into_any_element()
            }
            Block::CodeBlock { language, code } => self.code_block(language.as_deref(), code),
            Block::List {
                ordered,
                start,
                items,
            } => self.list(*ordered, *start, items, cx),
            Block::BlockQuote(blocks) => {
                let children: Vec<_> = blocks.iter().map(|b| self.block(b, cx)).collect();
                div()
                    .pl_3()
                    .border_l_2()
                    .border_color(self.theme.border_strong)
                    .text_color(self.theme.text_muted)
                    .child(v_flex().gap_2().children(children))
                    .into_any_element()
            }
            Block::Table { headers, rows } => self.table(headers, rows, cx),
            Block::Rule => div()
                .my_2()
                .h(px(1.))
                .w_full()
                .bg(self.theme.border)
                .into_any_element(),
        }
    }

    fn base_style(&self) -> HighlightStyle {
        HighlightStyle {
            color: Some(self.theme.text),
            ..Default::default()
        }
    }

    fn code_block(&mut self, language: Option<&str>, code: &str) -> AnyElement {
        div()
            .p_2()
            .rounded_tl(px(6.))
            .rounded_tr(px(6.))
            .rounded_bl(px(6.))
            .rounded_br(px(6.))
            .bg(self.theme.background)
            .border_1()
            .border_color(self.theme.border)
            .child(
                v_flex()
                    .gap_1()
                    .when_some(language, |el, language| {
                        el.child(
                            div()
                                .text_size(rems(0.65))
                                .text_color(self.theme.text_subtle)
                                .child(language.to_string()),
                        )
                    })
                    .child(
                        div()
                            .font_family(self.theme.mono_font.clone())
                            .text_size(rems(0.78))
                            .text_color(self.theme.text)
                            .child(code.trim_end().to_string()),
                    ),
            )
            .into_any_element()
    }

    fn list(&mut self, ordered: bool, start: u64, items: &[ListItem], cx: &App) -> AnyElement {
        let rows: Vec<_> = items
            .iter()
            .enumerate()
            .map(|(ix, item)| {
                let marker = match (ordered, item.checked) {
                    (_, Some(true)) => "☑".to_string(),
                    (_, Some(false)) => "☐".to_string(),
                    (true, None) => format!("{}.", start + ix as u64),
                    (false, None) => "•".to_string(),
                };
                let children: Vec<_> = item.blocks.iter().map(|b| self.block(b, cx)).collect();
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .w(px(18.))
                            .flex_none()
                            .text_color(self.theme.text_subtle)
                            .child(marker),
                    )
                    .child(v_flex().gap_1().flex_1().children(children))
                    .into_any_element()
            })
            .collect();

        v_flex().gap_1().children(rows).into_any_element()
    }

    fn table(
        &mut self,
        headers: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        cx: &App,
    ) -> AnyElement {
        let header_row = div()
            .flex()
            .flex_row()
            .gap_3()
            .pb_1()
            .border_b_1()
            .border_color(self.theme.border)
            .children(headers.iter().map(|cell| {
                div()
                    .flex_1()
                    .text_color(self.theme.text)
                    .child(self.inlines(cell, self.base_style(), cx))
            }));

        let body: Vec<_> = rows
            .iter()
            .map(|row| {
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .children(row.iter().map(|cell| {
                        div()
                            .flex_1()
                            .child(self.inlines(cell, self.base_style(), cx))
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .child(header_row)
            .children(body)
            .into_any_element()
    }

    /// Flatten inline nodes into one string plus styling and link ranges, then
    /// render as a single shaped text run.
    fn inlines(&mut self, inlines: &[Inline], base: HighlightStyle, _cx: &App) -> AnyElement {
        let mut build = Flattened::default();
        for inline in inlines {
            self.flatten(inline, base, &mut build);
        }

        if build.text.is_empty() {
            return div().into_any_element();
        }

        let text = SharedString::from(build.text);
        let styled = StyledText::new(text).with_highlights(build.highlights);

        if build.links.is_empty() {
            return styled.into_any_element();
        }

        let destinations: Vec<String> = build.links.iter().map(|(_, url)| url.clone()).collect();
        let ranges: Vec<Range<usize>> = build.links.into_iter().map(|(range, _)| range).collect();
        let id = self.next_id();

        InteractiveText::new(id, styled)
            .on_click(ranges, move |ix, _window, cx| {
                if let Some(url) = destinations.get(ix) {
                    cx.open_url(url);
                }
            })
            .into_any_element()
    }

    fn flatten(&self, inline: &Inline, style: HighlightStyle, out: &mut Flattened) {
        match inline {
            Inline::Text(text) => out.push(text, style),
            Inline::Code(code) => out.push(
                code,
                HighlightStyle {
                    color: Some(self.theme.accent),
                    background_color: Some(self.theme.surface_raised),
                    ..style
                },
            ),
            Inline::Emphasis(children) => {
                let style = HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..style
                };
                for child in children {
                    self.flatten(child, style, out);
                }
            }
            Inline::Strong(children) => {
                let style = HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..style
                };
                for child in children {
                    self.flatten(child, style, out);
                }
            }
            Inline::Strikethrough(children) => {
                let style = HighlightStyle {
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: style.color,
                    }),
                    ..style
                };
                for child in children {
                    self.flatten(child, style, out);
                }
            }
            Inline::Link { dest, children } => {
                let start = out.text.len();
                let style = HighlightStyle {
                    color: Some(self.theme.accent),
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(self.theme.accent),
                        wavy: false,
                    }),
                    ..style
                };
                for child in children {
                    self.flatten(child, style, out);
                }
                if out.text.len() > start {
                    out.links.push((start..out.text.len(), dest.clone()));
                }
            }
            // Images are not fetched; show the alt text so meaning survives.
            Inline::Image { alt, .. } => out.push(
                &format!("[{alt}]"),
                HighlightStyle {
                    color: Some(self.theme.text_subtle),
                    ..style
                },
            ),
            Inline::SoftBreak => out.push(" ", style),
            Inline::HardBreak => out.push("\n", style),
        }
    }
}

#[derive(Default)]
struct Flattened {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    links: Vec<(Range<usize>, String)>,
}

impl Flattened {
    fn push(&mut self, text: &str, style: HighlightStyle) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        self.highlights.push((start..self.text.len(), style));
    }
}
