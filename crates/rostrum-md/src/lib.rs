//! Markdown parsing for PR bodies and comments.
//!
//! Turns Markdown source into an owned tree of [`Block`] and [`Inline`] nodes
//! that a renderer can walk directly. The crate is pure: no I/O, no rendering
//! toolkit, no global state.
//!
//! ```
//! use rostrum_md::{Block, Document, GitHubContext, Inline, parse, parse_github};
//!
//! let document = parse("Hello *world*");
//! assert_eq!(document.blocks.len(), 1);
//!
//! let context = GitHubContext::new("zed-industries", "zed");
//! let document = parse_github("see #12", &context);
//! assert_eq!(
//!     document,
//!     Document {
//!         blocks: vec![Block::Paragraph(vec![
//!             Inline::Text("see ".into()),
//!             Inline::Link {
//!                 dest: "https://github.com/zed-industries/zed/issues/12".into(),
//!                 children: vec![Inline::Text("#12".into())],
//!             },
//!         ])],
//!     },
//! );
//! ```
//!
//! Two properties the tree guarantees, and which the renderer relies on:
//!
//! * Raw HTML never reaches the tree. HTML blocks and inline HTML tags are
//!   dropped during parsing, so no node can carry interpretable markup.
//! * Nesting is bounded. Containers deeper than 32 levels are flattened to
//!   their literal text, which keeps both parsing and the tree's recursive
//!   `Drop` off the stack cliff for pathological input.

mod ast;
mod github;
mod parser;

#[cfg(test)]
mod tests;

pub use ast::{Block, Document, GitHubContext, Inline, ListItem};

/// Parses Markdown into a renderable tree.
///
/// Enabled extensions: tables, strikethrough, task lists and footnotes, plus
/// GitHub's block-quote alerts. CommonMark autolinks (`<https://example.com>`)
/// are recognised; bare URLs are left as text.
pub fn parse(source: &str) -> Document {
    parser::parse(source)
}

/// Parses Markdown and expands GitHub shorthand into links.
///
/// `@login`, `#123` and `owner/repo#123` occurring in ordinary text become
/// [`Inline::Link`] nodes. Code spans, code blocks, image alt text and the
/// children of links already present in the source are never rewritten.
pub fn parse_github(source: &str, context: &GitHubContext) -> Document {
    let mut document = parser::parse(source);
    github::expand(&mut document, context);
    document
}
