//! Syntax colours for a whole file's diff lines.
//!
//! Uses `rostrum-diff`'s highlighter, the desktop's own, so both apps colour
//! code identically (syntect's `base16-ocean.dark`, legible on the app's
//! `#0f1115` background). The highlighter's parser is stateful across lines,
//! so each side of the diff is highlighted as one stream in file order — the
//! new side (context and added lines) and the old side (context and removed
//! lines) — rather than line by line, which keeps block comments and strings
//! coloured across lines within a hunk.

use std::sync::LazyLock;

use rostrum_diff::{DiffFile, HighlightSpan, Highlighter, LineKind, SpanStyle};

/// Loading syntect's syntax and theme dumps takes tens of milliseconds and a
/// few megabytes; it happens once, on first use, off the caller's thread.
static HIGHLIGHTER: LazyLock<Highlighter> = LazyLock::new(Highlighter::new);

/// Past this many lines a file is shown in the plain colour: highlighting it
/// would cost more than it helps on a phone.
const MAX_HIGHLIGHTED_LINES: usize = 5_000;

/// The colour for text with no syntax information.
pub(crate) fn plain_style() -> SpanStyle {
    HIGHLIGHTER.plain_style()
}

/// Highlight runs for every line of every hunk, indexed `[hunk][line]`.
pub(crate) fn file_highlights(file: &DiffFile) -> Vec<Vec<Vec<HighlightSpan>>> {
    let total: usize = file.hunks.iter().map(|hunk| hunk.lines.len()).sum();
    let mut out: Vec<Vec<Vec<HighlightSpan>>> = file
        .hunks
        .iter()
        .map(|hunk| vec![Vec::new(); hunk.lines.len()])
        .collect();
    if total > MAX_HIGHLIGHTED_LINES {
        let plain = plain_style();
        for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
            for (line_ix, line) in hunk.lines.iter().enumerate() {
                if !line.content.is_empty() {
                    out[hunk_ix][line_ix] = vec![HighlightSpan {
                        len: line.content.len(),
                        style: plain,
                    }];
                }
            }
        }
        return out;
    }

    // (hunk, line) positions of each side's stream, in file order.
    let mut new_side = Vec::new();
    let mut old_side = Vec::new();
    for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
        for (line_ix, line) in hunk.lines.iter().enumerate() {
            match line.kind {
                LineKind::Added => new_side.push((hunk_ix, line_ix)),
                LineKind::Removed => old_side.push((hunk_ix, line_ix)),
                LineKind::Context => {
                    new_side.push((hunk_ix, line_ix));
                    old_side.push((hunk_ix, line_ix));
                }
            }
        }
    }
    let text = |at: &(usize, usize)| file.hunks[at.0].lines[at.1].content.as_str();

    let new_text: Vec<&str> = new_side.iter().map(text).collect();
    let new_spans = HIGHLIGHTER.highlight_lines_for_path(&file.path, &new_text);
    for (at, spans) in new_side.iter().zip(new_spans) {
        out[at.0][at.1] = spans;
    }

    let old_path = file.previous_path.as_deref().unwrap_or(&file.path);
    let old_text: Vec<&str> = old_side.iter().map(text).collect();
    let old_spans = HIGHLIGHTER.highlight_lines_for_path(old_path, &old_text);
    for (at, spans) in old_side.iter().zip(old_spans) {
        // Context lines were coloured by the new side already; the old side
        // only kept the parser's state continuous across them.
        if file.hunks[at.0].lines[at.1].kind == LineKind::Removed {
            out[at.0][at.1] = spans;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use rostrum_diff::{FileStatus, PatchAvailability, parse_patch};

    use super::*;

    fn file(path: &str, patch: &str) -> DiffFile {
        DiffFile {
            path: path.into(),
            previous_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            hunks: parse_patch(patch).expect("patch"),
            availability: PatchAvailability::Present,
        }
    }

    #[test]
    fn every_line_is_covered_exactly() {
        let diff = file(
            "src/main.rs",
            "@@ -1,3 +1,3 @@\n fn main() {\n-    let a = 1;\n+    let a = \"two\";\n }\n",
        );
        let spans = file_highlights(&diff);
        for (hunk, hunk_spans) in diff.hunks.iter().zip(&spans) {
            for (line, line_spans) in hunk.lines.iter().zip(hunk_spans) {
                let covered: usize = line_spans.iter().map(|span| span.len).sum();
                assert_eq!(covered, line.content.len(), "{:?}", line.content);
            }
        }
        // A Rust keyword is coloured differently from plain text.
        let first = &spans[0][0];
        assert!(first.len() > 1, "{first:?}");
    }

    #[test]
    fn an_unknown_language_is_plain() {
        let diff = file("notes.unknownext", "@@ -1 +1 @@\n-a\n+b\n");
        let spans = file_highlights(&diff);
        assert_eq!(spans[0][0][0].style, plain_style());
        assert_eq!(spans[0][1][0].style, plain_style());
    }
}
