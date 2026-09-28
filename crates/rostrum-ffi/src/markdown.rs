//! Markdown flattened into a render-ready, non-recursive block list.
//!
//! `rostrum-md` parses into a tree (lists hold blocks, quotes hold blocks,
//! inlines nest). A recursive type does not cross UniFFI comfortably and a
//! `LazyColumn` wants a flat list anyway, so the tree is walked once here:
//! nesting becomes `quote_depth` and `list_depth` on each block, and inline
//! nesting becomes style flags on each span.

use rostrum_core::RepoId;

use crate::error::RostrumError;
use rostrum_md::{Block, GitHubContext, Inline, parse_github};

/// One renderable block. Blocks inside a list item or a quote carry the
/// nesting as depths rather than as children.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MdBlock {
    pub kind: MdBlockKind,
    /// Inline content. Empty for `Code`, `Rule`, `TableRow` and `Image`, which
    /// carry their content in the kind.
    pub spans: Vec<MdSpan>,
    /// How many block quotes enclose this block; draw that many bars.
    pub quote_depth: u32,
    /// How many lists enclose this block. A `ListItem` at depth 1 is a
    /// top-level bullet; any other block with a non-zero depth is a
    /// continuation paragraph of the item above it, indented to match.
    pub list_depth: u32,
}

/// What kind of block, with the data only that kind has.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum MdBlockKind {
    Paragraph,
    /// `level` is 1..=6.
    Heading {
        level: u8,
    },
    Code {
        /// The fence's info string, e.g. `rust`.
        language: Option<String>,
        code: String,
    },
    /// The first block of a list item; its spans are the item's first
    /// paragraph (empty when the item starts with something else).
    ListItem {
        ordered: bool,
        /// The item's ordinal for ordered lists; 0 for bullets.
        number: u64,
        /// Task-list checkbox state, when the item has one.
        checked: Option<bool>,
    },
    /// A thematic break.
    Rule,
    /// One row of a table; consecutive rows form the table.
    TableRow {
        cells: Vec<Vec<MdSpan>>,
        header: bool,
    },
    /// An image on a line of its own.
    Image {
        url: String,
        alt: String,
    },
}

/// A run of text with one style. Concatenating a block's spans gives its
/// plain text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MdSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Inline code: render monospace.
    pub code: bool,
    pub strike: bool,
    /// The span is (part of) a link to this URL. GitHub shorthand — `@login`,
    /// `#123`, `owner/repo#123` — arrives already expanded into links.
    pub link: Option<String>,
}

/// Parse `source` as GitHub-flavoured markdown in `repo`'s context (so `#12`
/// links to that repository) and flatten it.
pub(crate) fn render(source: &str, repo: &RepoId) -> Vec<MdBlock> {
    let context = GitHubContext::new(repo.owner(), repo.name());
    let document = parse_github(source, &context);
    let mut out = Vec::new();
    flatten_blocks(&document.blocks, Nesting::default(), &mut out);
    out
}

/// Render what the user is writing — the composer's Preview — exactly as the
/// timeline will show it once posted in `repo` (`owner/name`), so `#12` and
/// `@login` expand the same way before and after.
#[uniffi::export]
pub fn render_markdown(source: String, repo: String) -> Result<Vec<MdBlock>, RostrumError> {
    let repo: RepoId = repo
        .parse()
        .map_err(|err: rostrum_core::model::ParseRepoIdError| {
            RostrumError::invalid(err.to_string())
        })?;
    Ok(render(&source, &repo))
}

/// Where a block sits: inside how many quotes and lists.
#[derive(Clone, Copy, Default)]
struct Nesting {
    quote: u32,
    list: u32,
}

fn flatten_blocks(blocks: &[Block], nesting: Nesting, out: &mut Vec<MdBlock>) {
    for block in blocks {
        flatten_block(block, nesting, out);
    }
}

fn flatten_block(block: &Block, nesting: Nesting, out: &mut Vec<MdBlock>) {
    let push = |out: &mut Vec<MdBlock>, kind: MdBlockKind, spans: Vec<MdSpan>| {
        out.push(MdBlock {
            kind,
            spans,
            quote_depth: nesting.quote,
            list_depth: nesting.list,
        });
    };
    match block {
        Block::Paragraph(inlines) => match standalone_images(inlines) {
            Some(images) => {
                for (url, alt) in images {
                    push(out, MdBlockKind::Image { url, alt }, Vec::new());
                }
            }
            None => push(out, MdBlockKind::Paragraph, spans(inlines)),
        },
        Block::Heading { level, children } => {
            push(out, MdBlockKind::Heading { level: *level }, spans(children));
        }
        Block::CodeBlock { language, code } => push(
            out,
            MdBlockKind::Code {
                language: language.clone(),
                code: code.clone(),
            },
            Vec::new(),
        ),
        Block::List {
            ordered,
            start,
            items,
        } => {
            let inner = Nesting {
                quote: nesting.quote,
                list: nesting.list + 1,
            };
            for (offset, item) in items.iter().enumerate() {
                let number = if *ordered {
                    start.saturating_add(offset as u64)
                } else {
                    0
                };
                // The item's first paragraph rides on its marker; everything
                // after it is a continuation at the item's depth.
                let (first, rest) = match item.blocks.split_first() {
                    Some((Block::Paragraph(inlines), rest)) => (spans(inlines), rest),
                    _ => (Vec::new(), item.blocks.as_slice()),
                };
                out.push(MdBlock {
                    kind: MdBlockKind::ListItem {
                        ordered: *ordered,
                        number,
                        checked: item.checked,
                    },
                    spans: first,
                    quote_depth: inner.quote,
                    list_depth: inner.list,
                });
                flatten_blocks(rest, inner, out);
            }
        }
        Block::BlockQuote(blocks) => flatten_blocks(
            blocks,
            Nesting {
                quote: nesting.quote + 1,
                list: nesting.list,
            },
            out,
        ),
        Block::Table { headers, rows } => {
            push(
                out,
                MdBlockKind::TableRow {
                    cells: headers.iter().map(|cell| spans(cell)).collect(),
                    header: true,
                },
                Vec::new(),
            );
            for row in rows {
                push(
                    out,
                    MdBlockKind::TableRow {
                        cells: row.iter().map(|cell| spans(cell)).collect(),
                        header: false,
                    },
                    Vec::new(),
                );
            }
        }
        Block::Rule => push(out, MdBlockKind::Rule, Vec::new()),
    }
}

/// A paragraph made of nothing but images (and the whitespace between them),
/// as `(url, alt)` pairs; `None` if it holds anything else.
fn standalone_images(inlines: &[Inline]) -> Option<Vec<(String, String)>> {
    let mut images = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Image { dest, alt } => images.push((dest.clone(), alt.clone())),
            Inline::SoftBreak | Inline::HardBreak => {}
            Inline::Text(text) if text.trim().is_empty() => {}
            _ => return None,
        }
    }
    (!images.is_empty()).then_some(images)
}

/// The style accumulated from enclosing inline nodes.
#[derive(Clone, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    link: Option<String>,
}

fn spans(inlines: &[Inline]) -> Vec<MdSpan> {
    let mut out = Vec::new();
    walk(inlines, &Style::default(), &mut out);
    out
}

fn walk(inlines: &[Inline], style: &Style, out: &mut Vec<MdSpan>) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => push_span(out, text, style, false),
            Inline::Code(text) => push_span(out, text, style, true),
            Inline::Emphasis(children) => walk(
                children,
                &Style {
                    italic: true,
                    ..style.clone()
                },
                out,
            ),
            Inline::Strong(children) => walk(
                children,
                &Style {
                    bold: true,
                    ..style.clone()
                },
                out,
            ),
            Inline::Strikethrough(children) => walk(
                children,
                &Style {
                    strike: true,
                    ..style.clone()
                },
                out,
            ),
            Inline::Link { dest, children } => walk(
                children,
                &Style {
                    link: Some(dest.clone()),
                    ..style.clone()
                },
                out,
            ),
            // An image inside text becomes a link to it, labelled by its alt.
            Inline::Image { dest, alt } => {
                let label = if alt.trim().is_empty() { "image" } else { alt };
                push_span(
                    out,
                    label,
                    &Style {
                        link: Some(dest.clone()),
                        ..style.clone()
                    },
                    false,
                );
            }
            Inline::SoftBreak => push_span(out, " ", style, false),
            Inline::HardBreak => push_span(out, "\n", style, false),
        }
    }
}

/// Append text, merging into the previous span when the style is identical,
/// so a paragraph of plain words is one span rather than one per word.
fn push_span(out: &mut Vec<MdSpan>, text: &str, style: &Style, code: bool) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.bold == style.bold
        && last.italic == style.italic
        && last.strike == style.strike
        && last.code == code
        && last.link == style.link
    {
        last.text.push_str(text);
        return;
    }
    out.push(MdSpan {
        text: text.to_string(),
        bold: style.bold,
        italic: style.italic,
        code,
        strike: style.strike,
        link: style.link.clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(source: &str) -> Vec<MdBlock> {
        render(source, &"octo/repo".parse().expect("repo"))
    }

    fn plain(spans: &[MdSpan]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    fn span(text: &str) -> MdSpan {
        MdSpan {
            text: text.into(),
            bold: false,
            italic: false,
            code: false,
            strike: false,
            link: None,
        }
    }

    #[test]
    fn the_preview_export_renders_like_the_timeline() {
        let source = "Fixes #12, thanks @ada-lin\n\n- [x] done".to_string();
        assert_eq!(
            render_markdown(source.clone(), "octo/repo".into()).expect("renders"),
            md(&source)
        );
    }

    #[test]
    fn the_preview_export_rejects_a_malformed_repo() {
        assert!(matches!(
            render_markdown("hi".into(), "not a repo".into()),
            Err(RostrumError::InvalidInput { .. })
        ));
    }

    #[test]
    fn an_empty_body_has_no_blocks() {
        assert!(md("").is_empty());
        assert!(md("   \n\n").is_empty());
    }

    #[test]
    fn plain_words_are_one_span() {
        let blocks = md("Just some words\nacross lines.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, MdBlockKind::Paragraph);
        assert_eq!(blocks[0].spans, vec![span("Just some words across lines.")]);
        assert_eq!((blocks[0].quote_depth, blocks[0].list_depth), (0, 0));
    }

    #[test]
    fn inline_styles_nest_into_flags() {
        let blocks = md("a **bold *both*** `code` ~~gone~~ _it_");
        let spans = &blocks[0].spans;
        assert_eq!(plain(spans), "a bold both code gone it");
        let find = |text: &str| {
            spans
                .iter()
                .find(|span| span.text == text)
                .unwrap_or_else(|| panic!("no span {text:?} in {spans:?}"))
        };
        assert!(find("bold ").bold && !find("bold ").italic);
        assert!(find("both").bold && find("both").italic);
        assert!(find("code").code);
        assert!(find("gone").strike);
        assert!(find("it").italic);
    }

    #[test]
    fn links_mark_every_span_inside_them() {
        let blocks = md("see [the **docs**](https://example.com/docs) now");
        let spans = &blocks[0].spans;
        assert_eq!(plain(spans), "see the docs now");
        let linked: Vec<&MdSpan> = spans.iter().filter(|span| span.link.is_some()).collect();
        assert_eq!(linked.len(), 2);
        assert!(
            linked
                .iter()
                .all(|span| span.link.as_deref() == Some("https://example.com/docs"))
        );
        assert!(linked[1].bold);
    }

    #[test]
    fn github_shorthand_becomes_links_in_the_repositorys_context() {
        let blocks = md("fixes #12, thanks @octocat");
        let links: Vec<(&str, &str)> = blocks[0]
            .spans
            .iter()
            .filter_map(|span| span.link.as_deref().map(|link| (span.text.as_str(), link)))
            .collect();
        assert_eq!(
            links,
            vec![
                ("#12", "https://github.com/octo/repo/issues/12"),
                ("@octocat", "https://github.com/octocat"),
            ]
        );
    }

    #[test]
    fn hard_breaks_are_newlines() {
        let blocks = md("one  \ntwo");
        assert_eq!(plain(&blocks[0].spans), "one\ntwo");
    }

    #[test]
    fn headings_rules_and_code_blocks() {
        let blocks = md("## Title\n\n---\n\n```rust\nfn main() {}\n```\n");
        assert_eq!(blocks[0].kind, MdBlockKind::Heading { level: 2 });
        assert_eq!(plain(&blocks[0].spans), "Title");
        assert_eq!(blocks[1].kind, MdBlockKind::Rule);
        assert_eq!(
            blocks[2].kind,
            MdBlockKind::Code {
                language: Some("rust".into()),
                code: "fn main() {}\n".into()
            }
        );
        assert!(blocks[2].spans.is_empty());
    }

    #[test]
    fn nested_lists_carry_depth_and_numbers() {
        let blocks = md("3. three\n4. four\n   - inner\n   - [x] done\n");
        let items: Vec<(u32, &MdBlockKind, String)> = blocks
            .iter()
            .map(|block| (block.list_depth, &block.kind, plain(&block.spans)))
            .collect();
        assert_eq!(
            items,
            vec![
                (
                    1,
                    &MdBlockKind::ListItem {
                        ordered: true,
                        number: 3,
                        checked: None
                    },
                    "three".to_string()
                ),
                (
                    1,
                    &MdBlockKind::ListItem {
                        ordered: true,
                        number: 4,
                        checked: None
                    },
                    "four".to_string()
                ),
                (
                    2,
                    &MdBlockKind::ListItem {
                        ordered: false,
                        number: 0,
                        checked: None
                    },
                    "inner".to_string()
                ),
                (
                    2,
                    &MdBlockKind::ListItem {
                        ordered: false,
                        number: 0,
                        checked: Some(true)
                    },
                    "done".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_list_items_later_paragraphs_continue_at_its_depth() {
        let blocks = md("- first\n\n  more about it\n\n  ```\n  code\n  ```\n- second\n");
        assert!(matches!(blocks[0].kind, MdBlockKind::ListItem { .. }));
        assert_eq!(blocks[1].kind, MdBlockKind::Paragraph);
        assert_eq!(plain(&blocks[1].spans), "more about it");
        assert_eq!(blocks[1].list_depth, 1);
        assert!(matches!(blocks[2].kind, MdBlockKind::Code { .. }));
        assert_eq!(blocks[2].list_depth, 1);
        assert_eq!(plain(&blocks[3].spans), "second");
    }

    #[test]
    fn quotes_carry_their_depth_through_nested_blocks() {
        let blocks = md("> outer\n>\n> > inner\n>\n> - listed\n");
        assert_eq!(
            blocks
                .iter()
                .map(|block| (block.quote_depth, block.list_depth, plain(&block.spans)))
                .collect::<Vec<_>>(),
            vec![
                (1, 0, "outer".to_string()),
                (2, 0, "inner".to_string()),
                (1, 1, "listed".to_string()),
            ]
        );
    }

    #[test]
    fn tables_become_rows_with_a_header() {
        let blocks = md("| a | **b** |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n");
        assert_eq!(blocks.len(), 3);
        let MdBlockKind::TableRow { cells, header } = &blocks[0].kind else {
            panic!("expected a row, got {:?}", blocks[0].kind);
        };
        assert!(*header);
        assert_eq!(cells.len(), 2);
        assert!(cells[1][0].bold);
        let MdBlockKind::TableRow { cells, header } = &blocks[2].kind else {
            panic!("expected a row");
        };
        assert!(!*header);
        assert_eq!(plain(&cells[0]), "3");
    }

    #[test]
    fn an_image_alone_is_an_image_block() {
        let blocks = md("![screenshot](https://img/1.png)\n![](https://img/2.png)");
        assert_eq!(
            blocks.iter().map(|block| &block.kind).collect::<Vec<_>>(),
            vec![
                &MdBlockKind::Image {
                    url: "https://img/1.png".into(),
                    alt: "screenshot".into()
                },
                &MdBlockKind::Image {
                    url: "https://img/2.png".into(),
                    alt: String::new()
                },
            ]
        );
    }

    #[test]
    fn an_image_inside_text_is_a_link_to_it() {
        let blocks = md("before ![shot](https://img/1.png) after");
        assert_eq!(blocks[0].kind, MdBlockKind::Paragraph);
        let image = blocks[0]
            .spans
            .iter()
            .find(|span| span.text == "shot")
            .expect("image span");
        assert_eq!(image.link.as_deref(), Some("https://img/1.png"));
    }

    #[test]
    fn raw_html_never_reaches_the_blocks() {
        let blocks = md("<script>alert(1)</script>\n\nsafe <b>text</b>");
        let all: String = blocks.iter().map(|block| plain(&block.spans)).collect();
        assert!(!all.contains("<script>"), "{all}");
        assert!(!all.contains("<b>"), "{all}");
    }
}
