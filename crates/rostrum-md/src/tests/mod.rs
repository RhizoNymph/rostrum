//! Test suite for the crate.

use crate::ast::Inline;

mod github;
mod parser;

/// Shorthand for a literal text node.
fn text(value: &str) -> Inline {
    Inline::Text(value.to_owned())
}

/// Shorthand for an inline code node.
fn code(value: &str) -> Inline {
    Inline::Code(value.to_owned())
}
