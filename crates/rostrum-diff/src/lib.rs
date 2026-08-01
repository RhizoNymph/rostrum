//! Unified-diff parsing and syntax highlighting for pull request review.
//!
//! Two independent pieces:
//!
//! - [`parse_patch`] turns a GitHub per-file `patch` string into [`Hunk`]s whose
//!   [`DiffLine`]s carry reconstructed old/new line numbers.
//! - [`Highlighter`] turns lines of code into byte-exact [`HighlightSpan`] runs.
//!
//! # The critical invariant
//!
//! GitHub anchors review comments by `path` + `line` + `side`, where
//! [`Side::Right`] addresses the new file and [`Side::Left`] the old one.
//! [`DiffLine::anchor`] is the single place that mapping is implemented:
//! [`Added`](LineKind::Added) and [`Context`](LineKind::Context) lines anchor
//! with `new_line` on the right, [`Removed`](LineKind::Removed) lines with
//! `old_line` on the left, and a line missing its number is not commentable.
//! Getting this wrong puts comments on the wrong lines of real pull requests.
//!
//! ```
//! use rostrum_diff::{Side, parse_patch};
//!
//! let hunks = parse_patch("@@ -12,2 +20,3 @@ fn main() {\n ctx\n-old\n+new\n")?;
//! let lines = &hunks[0].lines;
//!
//! let anchor = lines[0].anchor("src/main.rs").expect("context is commentable");
//! assert_eq!((anchor.line, anchor.side), (20, Side::Right));
//!
//! let anchor = lines[1].anchor("src/main.rs").expect("removed is commentable");
//! assert_eq!((anchor.line, anchor.side), (13, Side::Left));
//!
//! let anchor = lines[2].anchor("src/main.rs").expect("added is commentable");
//! assert_eq!((anchor.line, anchor.side), (21, Side::Right));
//! # Ok::<(), rostrum_diff::DiffParseError>(())
//! ```

pub mod highlight;
pub mod model;
pub mod parse;

pub use highlight::{HighlightSpan, Highlighter, SpanStyle, SyntaxRef};
pub use model::{CommentAnchor, DiffFile, DiffLine, FileStatus, Hunk, LineKind, PatchAvailability};
pub use parse::{DiffParseError, parse_patch};
pub use rostrum_core::Side;
