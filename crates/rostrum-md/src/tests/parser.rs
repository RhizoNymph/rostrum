//! Structural tests for [`crate::parse`].

use crate::ast::{Block, Inline, ListItem};
use crate::parse;
use crate::parser::MAX_DEPTH;

use super::{code, text};

/// The blocks of a parsed document.
fn blocks(source: &str) -> Vec<Block> {
    parse(source).blocks
}

/// The single block a source is expected to produce.
fn one_block(source: &str) -> Block {
    let mut blocks = blocks(source);
    assert_eq!(blocks.len(), 1, "expected exactly one block for {source:?}");
    blocks.remove(0)
}

/// The inlines of a source expected to produce a single paragraph.
fn inlines(source: &str) -> Vec<Inline> {
    match one_block(source) {
        Block::Paragraph(children) => children,
        other => panic!("expected a paragraph for {source:?}, got {other:?}"),
    }
}

// --- empty and whitespace ---------------------------------------------------

#[test]
fn empty_input_produces_no_blocks() {
    assert!(parse("").is_empty());
}

#[test]
fn whitespace_only_input_produces_no_blocks() {
    for source in ["   ", "\n", "\n\n\n", "  \n\t\n  ", "\r\n\r\n"] {
        assert!(parse(source).is_empty(), "expected nothing for {source:?}");
    }
}

// --- paragraphs and inline nodes -------------------------------------------

#[test]
fn plain_paragraph() {
    assert_eq!(inlines("hello world"), vec![text("hello world")]);
}

#[test]
fn two_paragraphs() {
    assert_eq!(
        blocks("one\n\ntwo"),
        vec![
            Block::Paragraph(vec![text("one")]),
            Block::Paragraph(vec![text("two")]),
        ]
    );
}

#[test]
fn soft_break_between_lines() {
    assert_eq!(
        inlines("one\ntwo"),
        vec![text("one"), Inline::SoftBreak, text("two")]
    );
}

#[test]
fn hard_break_from_backslash() {
    assert_eq!(
        inlines("one\\\ntwo"),
        vec![text("one"), Inline::HardBreak, text("two")]
    );
}

#[test]
fn hard_break_from_trailing_spaces() {
    assert_eq!(
        inlines("one  \ntwo"),
        vec![text("one"), Inline::HardBreak, text("two")]
    );
}

#[test]
fn emphasis_and_strong() {
    assert_eq!(
        inlines("*em* and **strong**"),
        vec![
            Inline::Emphasis(vec![text("em")]),
            text(" and "),
            Inline::Strong(vec![text("strong")]),
        ]
    );
}

#[test]
fn strikethrough_extension_is_enabled() {
    assert_eq!(
        inlines("~~gone~~"),
        vec![Inline::Strikethrough(vec![text("gone")])]
    );
}

#[test]
fn inline_code_span() {
    assert_eq!(
        inlines("call `foo()` now"),
        vec![text("call "), code("foo()"), text(" now")]
    );
}

#[test]
fn emphasis_nested_in_strong() {
    assert_eq!(
        inlines("**bold *and* more**"),
        vec![Inline::Strong(vec![
            text("bold "),
            Inline::Emphasis(vec![text("and")]),
            text(" more"),
        ])]
    );
}

#[test]
fn emphasis_nested_in_link() {
    assert_eq!(
        inlines("[see *this* here](https://example.com)"),
        vec![Inline::Link {
            dest: "https://example.com".into(),
            children: vec![
                text("see "),
                Inline::Emphasis(vec![text("this")]),
                text(" here"),
            ],
        }]
    );
}

#[test]
fn link_inside_emphasis() {
    assert_eq!(
        inlines("*a [b](c) d*"),
        vec![Inline::Emphasis(vec![
            text("a "),
            Inline::Link {
                dest: "c".into(),
                children: vec![text("b")],
            },
            text(" d"),
        ])]
    );
}

#[test]
fn reference_link_resolves_destination() {
    assert_eq!(
        inlines("[label][ref]\n\n[ref]: https://example.com"),
        vec![Inline::Link {
            dest: "https://example.com".into(),
            children: vec![text("label")],
        }]
    );
}

#[test]
fn commonmark_autolink_is_a_link() {
    assert_eq!(
        inlines("<https://example.com>"),
        vec![Inline::Link {
            dest: "https://example.com".into(),
            children: vec![text("https://example.com")],
        }]
    );
}

#[test]
fn image_alt_is_flattened_child_text() {
    assert_eq!(
        inlines("![an *emphatic* logo](logo.png)"),
        vec![Inline::Image {
            dest: "logo.png".into(),
            alt: "an emphatic logo".into(),
        }]
    );
}

#[test]
fn image_inside_link() {
    assert_eq!(
        inlines("[![alt](i.png)](https://example.com)"),
        vec![Inline::Link {
            dest: "https://example.com".into(),
            children: vec![Inline::Image {
                dest: "i.png".into(),
                alt: "alt".into(),
            }],
        }]
    );
}

#[test]
fn adjacent_text_runs_are_merged() {
    // Entities and escapes split the event stream; the tree must not.
    assert_eq!(inlines(r"a &amp; b \* c"), vec![text("a & b * c")]);
}

// --- headings ---------------------------------------------------------------

#[test]
fn all_heading_levels() {
    for level in 1..=6u8 {
        let source = format!("{} title", "#".repeat(usize::from(level)));
        assert_eq!(
            one_block(&source),
            Block::Heading {
                level,
                children: vec![text("title")],
            }
        );
    }
}

#[test]
fn setext_heading() {
    assert_eq!(
        one_block("title\n====="),
        Block::Heading {
            level: 1,
            children: vec![text("title")],
        }
    );
}

#[test]
fn heading_keeps_inline_children() {
    assert_eq!(
        one_block("## a `b` **c**"),
        Block::Heading {
            level: 2,
            children: vec![
                text("a "),
                code("b"),
                text(" "),
                Inline::Strong(vec![text("c")]),
            ],
        }
    );
}

#[test]
fn empty_heading_is_kept() {
    assert_eq!(
        one_block("#"),
        Block::Heading {
            level: 1,
            children: vec![],
        }
    );
}

// --- code blocks ------------------------------------------------------------

#[test]
fn fenced_code_block_with_language() {
    assert_eq!(
        one_block("```rust\nfn main() {}\n```"),
        Block::CodeBlock {
            language: Some("rust".into()),
            code: "fn main() {}\n".into(),
        }
    );
}

#[test]
fn fenced_code_block_language_stops_at_attributes() {
    for source in ["```rust ignore\nx\n```", "```rust,no_run\nx\n```"] {
        assert_eq!(
            one_block(source),
            Block::CodeBlock {
                language: Some("rust".into()),
                code: "x\n".into(),
            }
        );
    }
}

#[test]
fn fenced_code_block_without_language() {
    assert_eq!(
        one_block("```\nplain\n```"),
        Block::CodeBlock {
            language: None,
            code: "plain\n".into(),
        }
    );
}

#[test]
fn indented_code_block_has_no_language() {
    assert_eq!(
        one_block("    indented\n    lines\n"),
        Block::CodeBlock {
            language: None,
            code: "indented\nlines\n".into(),
        }
    );
}

#[test]
fn code_block_contents_are_verbatim() {
    assert_eq!(
        one_block("```\n*not emphasis* `not code` <b>not html</b>\n```"),
        Block::CodeBlock {
            language: None,
            code: "*not emphasis* `not code` <b>not html</b>\n".into(),
        }
    );
}

// --- lists ------------------------------------------------------------------

#[test]
fn unordered_list() {
    assert_eq!(
        one_block("- a\n- b\n"),
        Block::List {
            ordered: false,
            start: 1,
            items: vec![
                ListItem {
                    checked: None,
                    blocks: vec![Block::Paragraph(vec![text("a")])],
                },
                ListItem {
                    checked: None,
                    blocks: vec![Block::Paragraph(vec![text("b")])],
                },
            ],
        }
    );
}

#[test]
fn ordered_list_records_start() {
    let Block::List {
        ordered,
        start,
        items,
    } = one_block("7. seven\n8. eight\n")
    else {
        panic!("expected a list");
    };
    assert!(ordered);
    assert_eq!(start, 7);
    assert_eq!(items.len(), 2);
}

#[test]
fn unordered_list_start_defaults_to_one() {
    let Block::List { ordered, start, .. } = one_block("* only\n") else {
        panic!("expected a list");
    };
    assert!(!ordered);
    assert_eq!(start, 1);
}

#[test]
fn loose_list_item_keeps_multiple_paragraphs() {
    let Block::List { items, .. } = one_block("1. a\n\n   still a\n\n2. b\n") else {
        panic!("expected a list");
    };
    assert_eq!(
        items[0].blocks,
        vec![
            Block::Paragraph(vec![text("a")]),
            Block::Paragraph(vec![text("still a")]),
        ]
    );
}

#[test]
fn nested_lists() {
    let Block::List { items, .. } = one_block("- outer\n  - inner\n    - deepest\n") else {
        panic!("expected a list");
    };
    assert_eq!(items.len(), 1);
    let [Block::Paragraph(head), Block::List { items: inner, .. }] = &items[0].blocks[..] else {
        panic!(
            "expected text then a nested list, got {:?}",
            items[0].blocks
        );
    };
    assert_eq!(head, &vec![text("outer")]);
    let [_, Block::List { items: deepest, .. }] = &inner[0].blocks[..] else {
        panic!("expected a second level of nesting");
    };
    assert_eq!(
        deepest[0].blocks,
        vec![Block::Paragraph(vec![text("deepest")])]
    );
}

#[test]
fn code_block_inside_list_item() {
    let Block::List { items, .. } = one_block("- item\n\n  ```sh\n  ls -l\n  ```\n") else {
        panic!("expected a list");
    };
    assert_eq!(
        items[0].blocks,
        vec![
            Block::Paragraph(vec![text("item")]),
            Block::CodeBlock {
                language: Some("sh".into()),
                code: "ls -l\n".into(),
            },
        ]
    );
}

#[test]
fn blockquote_inside_list_item() {
    let Block::List { items, .. } = one_block("- item\n\n  > quoted\n") else {
        panic!("expected a list");
    };
    assert_eq!(
        items[0].blocks,
        vec![
            Block::Paragraph(vec![text("item")]),
            Block::BlockQuote(vec![Block::Paragraph(vec![text("quoted")])]),
        ]
    );
}

#[test]
fn tight_list_item_inlines_become_a_paragraph() {
    let Block::List { items, .. } = one_block("- a `b` *c*\n") else {
        panic!("expected a list");
    };
    assert_eq!(
        items[0].blocks,
        vec![Block::Paragraph(vec![
            text("a "),
            code("b"),
            text(" "),
            Inline::Emphasis(vec![text("c")]),
        ])]
    );
}

// --- task lists -------------------------------------------------------------

#[test]
fn task_list_checkboxes() {
    let Block::List { items, .. } = one_block("- [x] done\n- [ ] todo\n- plain\n") else {
        panic!("expected a list");
    };
    assert_eq!(items[0].checked, Some(true));
    assert_eq!(items[1].checked, Some(false));
    assert_eq!(items[2].checked, None);
    assert_eq!(items[0].blocks, vec![Block::Paragraph(vec![text("done")])]);
    assert_eq!(items[1].blocks, vec![Block::Paragraph(vec![text("todo")])]);
}

#[test]
fn uppercase_task_marker_is_checked() {
    let Block::List { items, .. } = one_block("- [X] done\n") else {
        panic!("expected a list");
    };
    assert_eq!(items[0].checked, Some(true));
}

#[test]
fn nested_task_lists_do_not_steal_each_others_markers() {
    let Block::List { items, .. } = one_block("- [x] outer\n  - [ ] inner\n- [ ] sibling\n") else {
        panic!("expected a list");
    };
    assert_eq!(items[0].checked, Some(true));
    assert_eq!(items[1].checked, Some(false));
    let [_, Block::List { items: inner, .. }] = &items[0].blocks[..] else {
        panic!("expected a nested list");
    };
    assert_eq!(inner[0].checked, Some(false));
}

#[test]
fn task_item_with_only_a_nested_list_has_no_checkbox() {
    let Block::List { items, .. } = one_block("-\n  - [x] inner\n") else {
        panic!("expected a list");
    };
    assert_eq!(items[0].checked, None);
}

#[test]
fn task_item_with_empty_text() {
    let Block::List { items, .. } = one_block("- [ ]\n") else {
        panic!("expected a list");
    };
    assert_eq!(items[0].checked, Some(false));
}

#[test]
fn task_marker_in_a_loose_list() {
    let Block::List { items, .. } = one_block("- [x] first\n\n- [ ] second\n") else {
        panic!("expected a list");
    };
    assert_eq!(items[0].checked, Some(true));
    assert_eq!(items[1].checked, Some(false));
}

// --- block quotes -----------------------------------------------------------

#[test]
fn blockquote_with_several_blocks() {
    assert_eq!(
        one_block("> # head\n>\n> body\n>\n> - item\n"),
        Block::BlockQuote(vec![
            Block::Heading {
                level: 1,
                children: vec![text("head")],
            },
            Block::Paragraph(vec![text("body")]),
            Block::List {
                ordered: false,
                start: 1,
                items: vec![ListItem {
                    checked: None,
                    blocks: vec![Block::Paragraph(vec![text("item")])],
                }],
            },
        ])
    );
}

#[test]
fn nested_blockquotes() {
    assert_eq!(
        one_block("> outer\n>\n> > inner\n"),
        Block::BlockQuote(vec![
            Block::Paragraph(vec![text("outer")]),
            Block::BlockQuote(vec![Block::Paragraph(vec![text("inner")])]),
        ])
    );
}

#[test]
fn blockquote_with_code_block() {
    assert_eq!(
        one_block("> ```rs\n> let x = 1;\n> ```\n"),
        Block::BlockQuote(vec![Block::CodeBlock {
            language: Some("rs".into()),
            code: "let x = 1;\n".into(),
        }])
    );
}

#[test]
fn github_alert_quote_keeps_its_body() {
    assert_eq!(
        one_block("> [!NOTE]\n> pay attention\n"),
        Block::BlockQuote(vec![Block::Paragraph(vec![text("pay attention")])])
    );
}

// --- tables -----------------------------------------------------------------

#[test]
fn table_headers_and_rows() {
    assert_eq!(
        one_block("| a | b |\n| - | - |\n| 1 | 2 |\n| 3 | 4 |\n"),
        Block::Table {
            headers: vec![vec![text("a")], vec![text("b")]],
            rows: vec![
                vec![vec![text("1")], vec![text("2")]],
                vec![vec![text("3")], vec![text("4")]],
            ],
        }
    );
}

#[test]
fn table_alignment_is_ignored_but_cells_are_identical() {
    let aligned = one_block("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
    let plain = one_block("| a | b | c |\n| - | - | - |\n| 1 | 2 | 3 |\n");
    assert_eq!(aligned, plain);
    assert_eq!(
        aligned,
        Block::Table {
            headers: vec![vec![text("a")], vec![text("b")], vec![text("c")]],
            rows: vec![vec![vec![text("1")], vec![text("2")], vec![text("3")]]],
        }
    );
}

#[test]
fn table_cells_keep_inline_markup() {
    assert_eq!(
        one_block("| h |\n| - |\n| a **b** `c` [d](e) |\n"),
        Block::Table {
            headers: vec![vec![text("h")]],
            rows: vec![vec![vec![
                text("a "),
                Inline::Strong(vec![text("b")]),
                text(" "),
                code("c"),
                text(" "),
                Inline::Link {
                    dest: "e".into(),
                    children: vec![text("d")],
                },
            ]]],
        }
    );
}

#[test]
fn table_with_no_body_rows() {
    assert_eq!(
        one_block("| a | b |\n| - | - |\n"),
        Block::Table {
            headers: vec![vec![text("a")], vec![text("b")]],
            rows: vec![],
        }
    );
}

#[test]
fn table_with_empty_cells() {
    assert_eq!(
        one_block("| a | b |\n| - | - |\n|   | 2 |\n"),
        Block::Table {
            headers: vec![vec![text("a")], vec![text("b")]],
            rows: vec![vec![vec![], vec![text("2")]]],
        }
    );
}

// --- rules and footnotes ----------------------------------------------------

#[test]
fn thematic_breaks() {
    for source in ["---", "***", "___", "- - -"] {
        assert_eq!(one_block(source), Block::Rule, "for {source:?}");
    }
}

#[test]
fn footnote_reference_becomes_literal_text() {
    assert_eq!(
        blocks("body[^a]\n\n[^a]: note\n"),
        vec![
            Block::Paragraph(vec![text("body[^a]")]),
            Block::Paragraph(vec![text("[^a]: note")]),
        ]
    );
}

#[test]
fn footnote_definition_with_several_blocks() {
    assert_eq!(
        blocks("ref[^1]\n\n[^1]: first\n\n    ```\n    code\n    ```\n"),
        vec![
            Block::Paragraph(vec![text("ref[^1]")]),
            Block::Paragraph(vec![text("[^1]: first")]),
            Block::CodeBlock {
                language: None,
                code: "code\n".into(),
            },
        ]
    );
}

// --- raw HTML ---------------------------------------------------------------

#[test]
fn html_block_is_dropped() {
    assert!(parse("<div class=\"x\">\nhidden\n</div>\n").is_empty());
}

#[test]
fn script_block_is_dropped_entirely() {
    let document = parse("<script>alert('x')</script>\n");
    assert!(document.is_empty(), "got {document:?}");
}

#[test]
fn html_comment_is_dropped() {
    assert!(parse("<!-- a hidden comment -->\n").is_empty());
}

#[test]
fn inline_html_tags_are_dropped_but_their_text_survives() {
    assert_eq!(
        inlines("text <b>bold</b> more"),
        vec![text("text bold more")]
    );
}

#[test]
fn inline_html_never_reaches_the_tree_as_markup() {
    for child in inlines("a <img src=x onerror=y> b <br/> c") {
        let Inline::Text(value) = child else {
            panic!("expected only text, got {child:?}");
        };
        assert!(!value.contains('<'), "markup leaked: {value:?}");
        assert!(!value.contains("onerror"), "attributes leaked: {value:?}");
    }
}

#[test]
fn html_around_markdown_keeps_the_markdown() {
    assert_eq!(
        blocks("<details>\n<summary>s</summary>\n\nreal **content**\n\n</details>\n"),
        vec![Block::Paragraph(vec![
            text("real "),
            Inline::Strong(vec![text("content")]),
        ])]
    );
}

#[test]
fn escaped_angle_brackets_stay_literal_text() {
    assert_eq!(inlines(r"a \< b \> c"), vec![text("a < b > c")]);
}

// --- depth limiting ---------------------------------------------------------

/// Deepest chain of nested blocks in `blocks`.
fn block_depth(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::BlockQuote(inner) => 1 + block_depth(inner),
            Block::List { items, .. } => {
                1 + items
                    .iter()
                    .map(|item| block_depth(&item.blocks))
                    .max()
                    .unwrap_or(0)
            }
            _ => 1,
        })
        .max()
        .unwrap_or(0)
}

/// Deepest chain of nested inlines in `children`.
fn inline_depth(children: &[Inline]) -> usize {
    children
        .iter()
        .map(|child| match child {
            Inline::Emphasis(inner)
            | Inline::Strong(inner)
            | Inline::Strikethrough(inner)
            | Inline::Link {
                children: inner, ..
            } => 1 + inline_depth(inner),
            _ => 1,
        })
        .max()
        .unwrap_or(0)
}

fn contains_payload(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(children) => children
            .iter()
            .any(|child| child.plain_text().contains("payload")),
        Block::BlockQuote(inner) => contains_payload(inner),
        Block::List { items, .. } => items.iter().any(|item| contains_payload(&item.blocks)),
        _ => false,
    })
}

#[test]
fn deeply_nested_blockquotes_are_capped() {
    let source = format!("{}deep\n", "> ".repeat(500));
    let document = parse(&source);
    // The cap bounds containers; the innermost paragraph adds the final level.
    let depth = block_depth(&document.blocks);
    assert!(depth <= MAX_DEPTH + 1, "depth {depth} exceeded the cap");
    assert!(!document.is_empty());
}

#[test]
fn deeply_nested_blockquotes_keep_their_text() {
    let source = format!("{}payload\n", "> ".repeat(200));
    assert!(contains_payload(&parse(&source).blocks));
}

#[test]
fn deeply_nested_lists_are_capped_and_keep_their_text() {
    let mut source = String::new();
    for level in 0..300 {
        source.push_str(&" ".repeat(level * 2));
        source.push_str("- payload\n");
    }
    let document = parse(&source);
    let depth = block_depth(&document.blocks);
    assert!(depth <= MAX_DEPTH + 1, "depth {depth} exceeded the cap");
    assert!(contains_payload(&document.blocks));
}

#[test]
fn deeply_nested_emphasis_is_capped() {
    let source = format!("{}x{}", "*".repeat(400), "*".repeat(400));
    let document = parse(&source);
    let Some(Block::Paragraph(children)) = document.blocks.first() else {
        panic!("expected a paragraph, got {document:?}");
    };
    let depth = inline_depth(children);
    assert!(depth <= MAX_DEPTH + 1, "depth {depth} exceeded the cap");
}

#[test]
fn deeply_nested_links_do_not_overflow() {
    let source = format!("{}x{}", "[".repeat(500), "](u)".repeat(500));
    let _ = parse(&source);
}

#[test]
fn pathological_input_terminates() {
    for source in [
        "#".repeat(20_000),
        "> ".repeat(20_000),
        "- ".repeat(20_000),
        "`".repeat(20_000),
        "|".repeat(20_000),
        "~".repeat(20_000),
        "a".repeat(200_000),
        "<div>".repeat(20_000),
    ] {
        let _ = parse(&source);
    }
}

// --- integration ------------------------------------------------------------

#[test]
fn mixed_document() {
    let source = "\
# Title

Intro with *emphasis*, `code` and a [link](https://example.com).

- [x] first
- [ ] second
  - nested

> quoted
>
> ```rs
> fn f() {}
> ```

| col |
| --- |
| val |

---
";
    assert_eq!(
        blocks(source),
        vec![
            Block::Heading {
                level: 1,
                children: vec![text("Title")],
            },
            Block::Paragraph(vec![
                text("Intro with "),
                Inline::Emphasis(vec![text("emphasis")]),
                text(", "),
                code("code"),
                text(" and a "),
                Inline::Link {
                    dest: "https://example.com".into(),
                    children: vec![text("link")],
                },
                text("."),
            ]),
            Block::List {
                ordered: false,
                start: 1,
                items: vec![
                    ListItem {
                        checked: Some(true),
                        blocks: vec![Block::Paragraph(vec![text("first")])],
                    },
                    ListItem {
                        checked: Some(false),
                        blocks: vec![
                            Block::Paragraph(vec![text("second")]),
                            Block::List {
                                ordered: false,
                                start: 1,
                                items: vec![ListItem {
                                    checked: None,
                                    blocks: vec![Block::Paragraph(vec![text("nested")])],
                                }],
                            },
                        ],
                    },
                ],
            },
            Block::BlockQuote(vec![
                Block::Paragraph(vec![text("quoted")]),
                Block::CodeBlock {
                    language: Some("rs".into()),
                    code: "fn f() {}\n".into(),
                },
            ]),
            Block::Table {
                headers: vec![vec![text("col")]],
                rows: vec![vec![vec![text("val")]]],
            },
            Block::Rule,
        ]
    );
}

#[test]
fn plain_text_helper_flattens_nodes() {
    let children = inlines("a *b* `c` [d](e) ![f](g)");
    let joined: String = children.iter().map(Inline::plain_text).collect();
    assert_eq!(joined, "a b c d f");
}
