//! Lowers a `pulldown-cmark` event stream into the owned [`Document`] tree.
//!
//! The event stream is flat and guarantees balanced `Start`/`End` pairs, so the
//! builder is a straightforward recursive descent over it. Two properties are
//! load-bearing:
//!
//! * **Nesting is capped.** Recursion is bounded by [`MAX_DEPTH`]; past that a
//!   subtree is flattened to its literal text instead of being descended into,
//!   so pathological input cannot blow the stack — neither here nor in the
//!   recursive `Drop` of the resulting tree.
//! * **HTML never survives.** Raw HTML blocks and inline HTML tags are dropped
//!   outright. The tree can therefore never carry anything a renderer might
//!   interpret as markup; text inside inline HTML tags is still kept, because
//!   `pulldown-cmark` reports it as ordinary `Text`.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser as CmarkParser, Tag};

use crate::ast::{Block, Document, Inline, ListItem};

/// Maximum container nesting the builder will descend into.
pub(crate) const MAX_DEPTH: usize = 32;

/// GFM-ish extension set.
///
/// `pulldown-cmark` 0.13 has no separate autolink flag: CommonMark autolinks
/// (`<https://example.com>`) are always recognised, and bare-URL linkification
/// is not implemented upstream. `ENABLE_GFM` covers the remaining GitHub
/// divergences (alert-style block quotes).
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// Parses `source` into a renderable tree.
pub(crate) fn parse(source: &str) -> Document {
    let events: Vec<Event<'_>> = CmarkParser::new_ext(source, options()).collect();
    let mut builder = TreeBuilder {
        events: events.into_iter().peekable(),
        pending_task: None,
    };
    Document {
        blocks: builder.blocks(0),
    }
}

struct TreeBuilder<'a> {
    events: std::iter::Peekable<std::vec::IntoIter<Event<'a>>>,
    /// Set by a `TaskListMarker` event, consumed by the enclosing list item.
    pending_task: Option<bool>,
}

/// Events that open (or are) a block-level node.
fn is_block_level(event: &Event<'_>) -> bool {
    match event {
        Event::Rule => true,
        Event::Start(tag) => matches!(
            tag,
            Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::HtmlBlock
                | Tag::List(_)
                | Tag::Item
                | Tag::FootnoteDefinition(_)
                | Tag::DefinitionList
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
                | Tag::Table(_)
                | Tag::TableHead
                | Tag::TableRow
                | Tag::TableCell
                | Tag::MetadataBlock(_)
        ),
        _ => false,
    }
}

/// Appends `text`, merging into the previous node when it is also text.
fn push_text(out: &mut Vec<Inline>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text(previous)) = out.last_mut() {
        previous.push_str(text);
    } else {
        out.push(Inline::Text(text.to_owned()));
    }
}

impl<'a> TreeBuilder<'a> {
    /// Consumes the `End` closing the current container, if it is next.
    fn end_container(&mut self) {
        if matches!(self.events.peek(), Some(Event::End(_))) {
            self.events.next();
        }
    }

    /// Consumes events up to (but not including) the `End` of the current
    /// container. Iterative, so it is safe at any input depth.
    fn skip_children(&mut self) {
        loop {
            match self.events.peek() {
                None | Some(Event::End(_)) => return,
                Some(Event::Start(_)) => {
                    self.events.next();
                    self.skip_subtree();
                }
                _ => {
                    self.events.next();
                }
            }
        }
    }

    /// Consumes events through the `End` matching an already-consumed `Start`.
    fn skip_subtree(&mut self) {
        let mut depth = 1usize;
        for event in self.events.by_ref() {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// Like [`Self::skip_subtree`], but collects the literal text it passes.
    fn flatten_subtree(&mut self) -> String {
        let mut out = String::new();
        let mut depth = 1usize;
        for event in self.events.by_ref() {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return out;
                    }
                }
                Event::Text(text)
                | Event::Code(text)
                | Event::InlineMath(text)
                | Event::DisplayMath(text) => out.push_str(&text),
                Event::SoftBreak | Event::HardBreak => out.push(' '),
                _ => {}
            }
        }
        out
    }

    /// Reads blocks until the current container closes or the stream ends.
    ///
    /// Inline events encountered directly at block level (as happens inside
    /// tight list items) are gathered into an implicit paragraph.
    fn blocks(&mut self, depth: usize) -> Vec<Block> {
        let mut out = Vec::new();
        loop {
            let block_level = match self.events.peek() {
                None | Some(Event::End(_)) => break,
                Some(event) => is_block_level(event),
            };
            if block_level {
                self.block(depth, &mut out);
            } else {
                let children = self.inlines(depth + 1);
                if !children.is_empty() {
                    out.push(Block::Paragraph(children));
                }
            }
        }
        out
    }

    /// Reads exactly one block-level event, appending zero or more blocks.
    fn block(&mut self, depth: usize, out: &mut Vec<Block>) {
        match self.events.next() {
            Some(Event::Rule) => out.push(Block::Rule),
            Some(Event::Start(tag)) => self.container(tag, depth, out),
            _ => {}
        }
    }

    /// Handles a block-level `Start` whose event has already been consumed.
    fn container(&mut self, tag: Tag<'a>, depth: usize, out: &mut Vec<Block>) {
        if depth >= MAX_DEPTH {
            let text = self.flatten_subtree();
            if !text.trim().is_empty() {
                out.push(Block::Paragraph(vec![Inline::Text(text)]));
            }
            return;
        }

        match tag {
            Tag::Paragraph => {
                let children = self.inlines(depth + 1);
                self.end_container();
                if !children.is_empty() {
                    out.push(Block::Paragraph(children));
                }
            }
            Tag::Heading { level, .. } => {
                let children = self.inlines(depth + 1);
                self.end_container();
                out.push(Block::Heading {
                    level: level as u8,
                    children,
                });
            }
            Tag::BlockQuote(_) => {
                let blocks = self.blocks(depth + 1);
                self.end_container();
                if !blocks.is_empty() {
                    out.push(Block::BlockQuote(blocks));
                }
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Indented => None,
                    // Info strings carry extra attributes after the language,
                    // separated by whitespace (GitHub) or a comma (mdBook).
                    CodeBlockKind::Fenced(info) => info
                        .split(|character: char| character.is_whitespace() || character == ',')
                        .find(|token| !token.is_empty())
                        .map(str::to_owned),
                };
                let mut code = String::new();
                loop {
                    match self.events.peek() {
                        None | Some(Event::End(_)) => break,
                        _ => match self.events.next() {
                            Some(Event::Text(text)) => code.push_str(&text),
                            Some(Event::Start(_)) => self.skip_subtree(),
                            _ => {}
                        },
                    }
                }
                self.end_container();
                out.push(Block::CodeBlock { language, code });
            }
            Tag::List(first) => {
                let ordered = first.is_some();
                let start = first.unwrap_or(1);
                let mut items = Vec::new();
                loop {
                    match self.events.peek() {
                        Some(Event::Start(Tag::Item)) => {
                            self.events.next();
                            items.push(self.list_item(depth + 1));
                        }
                        None | Some(Event::End(_)) => break,
                        Some(Event::Start(_)) => {
                            self.events.next();
                            self.skip_subtree();
                        }
                        _ => {
                            self.events.next();
                        }
                    }
                }
                self.end_container();
                out.push(Block::List {
                    ordered,
                    start,
                    items,
                });
            }
            Tag::Table(_) => {
                let mut headers = Vec::new();
                let mut rows = Vec::new();
                loop {
                    match self.events.peek() {
                        Some(Event::Start(Tag::TableHead)) => {
                            self.events.next();
                            headers = self.table_row(depth + 1);
                        }
                        Some(Event::Start(Tag::TableRow)) => {
                            self.events.next();
                            rows.push(self.table_row(depth + 1));
                        }
                        None | Some(Event::End(_)) => break,
                        Some(Event::Start(_)) => {
                            self.events.next();
                            self.skip_subtree();
                        }
                        _ => {
                            self.events.next();
                        }
                    }
                }
                self.end_container();
                out.push(Block::Table { headers, rows });
            }
            Tag::FootnoteDefinition(label) => {
                let mut blocks = self.blocks(depth + 1);
                self.end_container();
                let marker = format!("[^{label}]: ");
                match blocks.first_mut() {
                    Some(Block::Paragraph(children)) => match children.first_mut() {
                        Some(Inline::Text(text)) => text.insert_str(0, &marker),
                        _ => children.insert(0, Inline::Text(marker)),
                    },
                    _ => blocks.insert(0, Block::Paragraph(vec![Inline::Text(marker)])),
                }
                out.append(&mut blocks);
            }
            // Raw HTML is dropped wholesale: see the module docs.
            Tag::HtmlBlock => {
                self.skip_children();
                self.end_container();
            }
            // Anything else (stray table parts, extensions we do not enable,
            // inline tags that cannot reach here) is discarded intact.
            _ => self.skip_subtree(),
        }
    }

    /// Reads one list item whose `Start` has already been consumed.
    fn list_item(&mut self, depth: usize) -> ListItem {
        self.pending_task = None;
        let mut blocks = Vec::new();
        let mut checked = None;
        let mut first = true;

        if depth < MAX_DEPTH {
            loop {
                let block_level = match self.events.peek() {
                    None | Some(Event::End(_)) => break,
                    Some(event) => is_block_level(event),
                };
                if block_level {
                    self.block(depth, &mut blocks);
                } else {
                    let children = self.inlines(depth + 1);
                    if !children.is_empty() {
                        blocks.push(Block::Paragraph(children));
                    }
                }
                // The checkbox always lives in the item's first block, before
                // any nested list could reset `pending_task`.
                if first {
                    checked = self.pending_task.take();
                    first = false;
                }
            }
        } else {
            let text = self.flatten_subtree();
            if !text.trim().is_empty() {
                blocks.push(Block::Paragraph(vec![Inline::Text(text)]));
            }
            self.pending_task = None;
            return ListItem { checked, blocks };
        }

        self.pending_task = None;
        self.end_container();
        ListItem { checked, blocks }
    }

    /// Reads the cells of a table head/row whose `Start` was consumed.
    fn table_row(&mut self, depth: usize) -> Vec<Vec<Inline>> {
        let mut cells = Vec::new();
        loop {
            match self.events.peek() {
                Some(Event::Start(Tag::TableCell)) => {
                    self.events.next();
                    let children = self.inlines(depth + 1);
                    self.end_container();
                    cells.push(children);
                }
                None | Some(Event::End(_)) => break,
                Some(Event::Start(_)) => {
                    self.events.next();
                    self.skip_subtree();
                }
                _ => {
                    self.events.next();
                }
            }
        }
        self.end_container();
        cells
    }

    /// Reads inline nodes until the current container closes, the stream ends,
    /// or a block-level event appears.
    fn inlines(&mut self, depth: usize) -> Vec<Inline> {
        let mut out = Vec::new();
        loop {
            match self.events.peek() {
                None | Some(Event::End(_)) => break,
                Some(event) => {
                    if is_block_level(event) {
                        break;
                    }
                }
            }
            let Some(event) = self.events.next() else {
                break;
            };
            match event {
                Event::Text(text) => push_text(&mut out, &text),
                Event::Code(code) => out.push(Inline::Code(code.into_string())),
                Event::InlineMath(math) | Event::DisplayMath(math) => {
                    out.push(Inline::Code(math.into_string()));
                }
                Event::FootnoteReference(label) => push_text(&mut out, &format!("[^{label}]")),
                Event::SoftBreak => out.push(Inline::SoftBreak),
                Event::HardBreak => out.push(Inline::HardBreak),
                Event::TaskListMarker(checked) => self.pending_task = Some(checked),
                Event::Start(tag) => self.inline_container(tag, depth, &mut out),
                // Raw HTML is dropped; see the module docs.
                Event::Html(_) | Event::InlineHtml(_) => {}
                Event::Rule | Event::End(_) => break,
            }
        }
        out
    }

    /// Handles an inline `Start` whose event has already been consumed.
    fn inline_container(&mut self, tag: Tag<'a>, depth: usize, out: &mut Vec<Inline>) {
        if depth >= MAX_DEPTH {
            let text = self.flatten_subtree();
            push_text(out, &text);
            return;
        }

        match tag {
            Tag::Emphasis => {
                let children = self.inlines(depth + 1);
                self.end_container();
                if !children.is_empty() {
                    out.push(Inline::Emphasis(children));
                }
            }
            Tag::Strong => {
                let children = self.inlines(depth + 1);
                self.end_container();
                if !children.is_empty() {
                    out.push(Inline::Strong(children));
                }
            }
            Tag::Strikethrough => {
                let children = self.inlines(depth + 1);
                self.end_container();
                if !children.is_empty() {
                    out.push(Inline::Strikethrough(children));
                }
            }
            Tag::Link { dest_url, .. } => {
                let children = self.inlines(depth + 1);
                self.end_container();
                out.push(Inline::Link {
                    dest: dest_url.into_string(),
                    children,
                });
            }
            Tag::Image { dest_url, .. } => {
                let alt = self.flatten_subtree();
                out.push(Inline::Image {
                    dest: dest_url.into_string(),
                    alt,
                });
            }
            // Extensions we do not enable, but whose text is worth keeping.
            Tag::Superscript | Tag::Subscript => {
                let children = self.inlines(depth + 1);
                self.end_container();
                out.extend(children);
            }
            _ => self.skip_subtree(),
        }
    }
}
