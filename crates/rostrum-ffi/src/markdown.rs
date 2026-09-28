//! Markdown flattened into a render-ready, non-recursive block list.
//!
//! `rostrum-md` parses into a tree (lists hold blocks, quotes hold blocks,
//! inlines nest). A recursive type does not cross UniFFI comfortably and a
//! `LazyColumn` wants a flat list anyway, so the tree is walked once here:
//! nesting becomes `quote_depth` and `list_depth` on each block, and inline
//! nesting becomes style flags on each span.

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
    Heading { level: u8 },
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
    Image { url: String, alt: String },
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
