//! The renderable Markdown tree.
//!
//! Every node is fully owned so the tree can outlive the source text and be
//! handed to a renderer on another thread. Nothing here is aware of how it will
//! be drawn.

/// A span-level node.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    /// Literal text. Never contains markup a renderer should interpret.
    Text(String),
    /// An inline code span.
    Code(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    Link {
        dest: String,
        children: Vec<Inline>,
    },
    Image {
        dest: String,
        alt: String,
    },
    /// A line break in the source that a renderer may collapse to a space.
    SoftBreak,
    /// An explicit line break the renderer must honour.
    HardBreak,
}

impl Inline {
    /// Concatenates the literal text carried by this node and its descendants.
    ///
    /// Code spans contribute their contents, breaks contribute a single space,
    /// and images contribute their alt text.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        self.write_plain_text(&mut out);
        out
    }

    fn write_plain_text(&self, out: &mut String) {
        match self {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children)
            | Inline::Link { children, .. } => {
                for child in children {
                    child.write_plain_text(out);
                }
            }
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::SoftBreak | Inline::HardBreak => out.push(' '),
        }
    }
}

/// One entry of a [`Block::List`].
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    /// `Some(true)`/`Some(false)` for task-list checkboxes, `None` otherwise.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// A block-level node.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        /// Always in `1..=6`.
        level: u8,
        children: Vec<Inline>,
    },
    CodeBlock {
        /// The first word of the info string, if the block was fenced with one.
        language: Option<String>,
        code: String,
    },
    List {
        ordered: bool,
        /// The first ordinal; `1` for unordered lists.
        start: u64,
        items: Vec<ListItem>,
    },
    BlockQuote(Vec<Block>),
    /// A table. Column alignment is deliberately not preserved.
    Table {
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// A thematic break.
    Rule,
}

/// A parsed Markdown document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    /// Returns `true` when the document produced no renderable blocks.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// Repository context used to expand GitHub shorthand into links.
#[derive(Clone, Debug, PartialEq)]
pub struct GitHubContext {
    pub owner: String,
    pub repo: String,
}

impl GitHubContext {
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
        }
    }
}
