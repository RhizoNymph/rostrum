//! Best-effort syntax highlighting for diff lines.
//!
//! Produces, per line, a list of [`HighlightSpan`]s whose lengths **exactly
//! cover that line's UTF-8 bytes**. The consumer turns each span into a gpui
//! `TextRun`, and gpui requires runs to tile the string with no gaps and no
//! overlaps — a span list that is one byte short paints the wrong glyphs.
//!
//! Highlighting is best-effort by design. An unknown language, an unparseable
//! line, or a syntax that bails mid-file degrades to a single unstyled span; it
//! never errors and never panics.

use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// The syntect theme used for all highlighting.
const THEME_NAME: &str = "base16-ocean.dark";

/// Fallback foreground when the theme declares none (`base16-ocean.dark`'s own
/// foreground, so the fallback is invisible in practice).
const FALLBACK_FOREGROUND: SpanStyle = SpanStyle {
    r: 0xc0,
    g: 0xc5,
    b: 0xce,
    bold: false,
    italic: false,
};

/// The visual style of one run of characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpanStyle {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub bold: bool,
    pub italic: bool,
}

/// A run of styled characters within a single line.
///
/// `len` is a count of **UTF-8 bytes**, not chars, matching gpui's `TextRun`.
/// Spans for a line appear in order and their lengths sum to the line's byte
/// length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HighlightSpan {
    pub len: usize,
    pub style: SpanStyle,
}

/// An opaque handle to a syntax definition owned by a [`Highlighter`].
///
/// The lifetime ties the handle to the borrow of the `Highlighter` that produced
/// it, so it cannot outlive the syntax set it indexes into.
#[derive(Clone, Copy, Debug)]
pub struct SyntaxRef<'a> {
    inner: &'a SyntaxReference,
}

impl SyntaxRef<'_> {
    /// The syntax's display name, e.g. `"Rust"`.
    pub fn name(&self) -> &str {
        &self.inner.name
    }
}

/// Owns syntect's syntax and theme data.
///
/// [`Highlighter::new`] loads syntect's bundled syntax and theme dumps, which
/// takes on the order of tens of milliseconds and a few megabytes. **Construct
/// one and share it** — never build one per file, and never inside `render`.
pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
    /// The theme's default foreground, used for unstyled and fallback spans.
    plain: SpanStyle,
}

impl Highlighter {
    /// Load syntect's default syntaxes and a dark theme.
    ///
    /// This is the slow constructor described on the type; call it once.
    pub fn new() -> Self {
        // The `newlines` variant expects each line to be fed with its trailing
        // `\n`; `highlight_lines` appends one and clips it back off, which is
        // more reliable than the `nonewlines` syntaxes' rewritten regexes.
        let syntaxes = SyntaxSet::load_defaults_newlines();

        let mut themes = ThemeSet::load_defaults();
        let theme = themes.themes.remove(THEME_NAME).unwrap_or_else(|| {
            tracing::warn!(
                theme = THEME_NAME,
                "bundled theme missing; using a bare theme"
            );
            Theme::default()
        });

        let plain = theme
            .settings
            .foreground
            .map(|c| SpanStyle {
                r: c.r,
                g: c.g,
                b: c.b,
                bold: false,
                italic: false,
            })
            .unwrap_or(FALLBACK_FOREGROUND);

        Self {
            syntaxes,
            theme,
            plain,
        }
    }

    /// The style applied to text with no syntax information.
    pub fn plain_style(&self) -> SpanStyle {
        self.plain
    }

    /// Pick a syntax from a file path's extension, or `None` if unknown.
    ///
    /// Falls back to matching the whole file name for extensionless files such
    /// as `Makefile` and `.gitignore`, which syntect registers by name.
    pub fn syntax_for_path(&self, path: &str) -> Option<SyntaxRef<'_>> {
        let name = Path::new(path).file_name()?.to_str()?;
        let by_extension = Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| self.syntaxes.find_syntax_by_extension(ext));

        by_extension
            .or_else(|| self.syntaxes.find_syntax_by_extension(name))
            .map(|inner| SyntaxRef { inner })
    }

    /// Highlight `lines`, returning one span list per input line.
    ///
    /// The returned `Vec` always has `lines.len()` entries, and entry `i`'s span
    /// lengths sum to exactly `lines[i].len()` (so an empty line yields an empty
    /// span list). Lines are highlighted in order because syntect's parser is
    /// stateful across lines — passing a file's lines out of order or in slices
    /// produces wrong, though still byte-exact, colours.
    pub fn highlight_lines(
        &self,
        syntax: SyntaxRef<'_>,
        lines: &[&str],
    ) -> Vec<Vec<HighlightSpan>> {
        let mut highlighter = HighlightLines::new(syntax.inner, &self.theme);
        let mut buf = String::new();
        let mut out = Vec::with_capacity(lines.len());

        for line in lines {
            out.push(self.spans_for_line(&mut highlighter, line, &mut buf));
        }
        out
    }

    /// Highlight `lines` using the syntax implied by `path`, degrading to a
    /// single unstyled span per line when the language is unknown.
    pub fn highlight_lines_for_path(&self, path: &str, lines: &[&str]) -> Vec<Vec<HighlightSpan>> {
        match self.syntax_for_path(path) {
            Some(syntax) => self.highlight_lines(syntax, lines),
            None => {
                tracing::debug!(path, "no syntax for path; rendering unstyled");
                lines.iter().map(|line| self.plain_spans(line)).collect()
            }
        }
    }

    /// A single span covering the whole line, or none for an empty line.
    fn plain_spans(&self, line: &str) -> Vec<HighlightSpan> {
        if line.is_empty() {
            Vec::new()
        } else {
            vec![HighlightSpan {
                len: line.len(),
                style: self.plain,
            }]
        }
    }

    fn spans_for_line(
        &self,
        highlighter: &mut HighlightLines<'_>,
        line: &str,
        buf: &mut String,
    ) -> Vec<HighlightSpan> {
        buf.clear();
        buf.push_str(line);
        buf.push('\n');

        let ranges = match highlighter.highlight_line(buf, &self.syntaxes) {
            Ok(ranges) => ranges,
            Err(err) => {
                tracing::debug!(error = %err, "syntax highlighting failed; rendering unstyled");
                return self.plain_spans(line);
            }
        };

        // Clip back to the original line: the last range covers the `\n` we
        // appended, and nothing downstream knows about that byte.
        let mut spans: Vec<HighlightSpan> = Vec::with_capacity(ranges.len());
        let mut covered = 0usize;
        for (style, text) in ranges {
            if covered >= line.len() {
                break;
            }
            let len = text.len().min(line.len() - covered);
            if len == 0 {
                continue;
            }
            spans.push(HighlightSpan {
                len,
                style: SpanStyle {
                    r: style.foreground.r,
                    g: style.foreground.g,
                    b: style.foreground.b,
                    bold: style.font_style.contains(FontStyle::BOLD),
                    italic: style.font_style.contains(FontStyle::ITALIC),
                },
            });
            covered += len;
        }

        // Belt and braces: if syntect ever returns short, pad rather than hand
        // gpui a run list that does not tile the line.
        if covered < line.len() {
            spans.push(HighlightSpan {
                len: line.len() - covered,
                style: self.plain,
            });
        }

        spans
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("syntaxes", &self.syntaxes.syntaxes().len())
            .field("theme", &self.theme.name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building a `Highlighter` is slow, so the tests share one.
    fn highlighter() -> &'static Highlighter {
        static HIGHLIGHTER: std::sync::OnceLock<Highlighter> = std::sync::OnceLock::new();
        HIGHLIGHTER.get_or_init(Highlighter::new)
    }

    /// The invariant every test in this module exists to protect.
    fn assert_exact_coverage(lines: &[&str], spans: &[Vec<HighlightSpan>]) {
        assert_eq!(
            spans.len(),
            lines.len(),
            "one span list per input line is required"
        );
        for (line, line_spans) in lines.iter().zip(spans) {
            let total: usize = line_spans.iter().map(|s| s.len).sum();
            assert_eq!(
                total,
                line.len(),
                "spans for {line:?} must cover exactly {} bytes, got {total}",
                line.len()
            );
            assert!(
                line_spans.iter().all(|s| s.len > 0),
                "zero-length spans are not valid gpui runs: {line_spans:?}"
            );

            // Every span boundary must land on a char boundary, or slicing the
            // line by span offsets would panic downstream.
            let mut offset = 0usize;
            for span in line_spans {
                offset += span.len;
                assert!(
                    line.is_char_boundary(offset),
                    "span boundary {offset} splits a character in {line:?}"
                );
            }
        }
    }

    fn rust(lines: &[&str]) -> Vec<Vec<HighlightSpan>> {
        let h = highlighter();
        let syntax = h
            .syntax_for_path("src/main.rs")
            .expect("rust syntax exists");
        h.highlight_lines(syntax, lines)
    }

    // -- syntax lookup -------------------------------------------------------

    #[test]
    fn finds_syntaxes_by_extension() {
        let h = highlighter();
        for path in [
            "src/main.rs",
            "crates/a/src/lib.rs",
            "index.js",
            "styles.css",
            "data.json",
            "README.md",
            "script.py",
            "main.c",
        ] {
            assert!(
                h.syntax_for_path(path).is_some(),
                "expected a syntax for {path}"
            );
        }
    }

    #[test]
    fn rust_files_get_the_rust_syntax() {
        let h = highlighter();
        let syntax = h.syntax_for_path("a/b/c.rs").expect("rust syntax exists");
        assert_eq!(syntax.name(), "Rust");
    }

    #[test]
    fn unknown_extensions_yield_no_syntax() {
        let h = highlighter();
        for path in [
            "a/b/thing.qqzz",
            "no-extension-here",
            "",
            "trailing/slash/",
            "archive.tar.wat",
        ] {
            assert!(
                h.syntax_for_path(path).is_none(),
                "expected no syntax for {path:?}"
            );
        }
    }

    #[test]
    fn extensionless_files_match_by_name() {
        let h = highlighter();
        assert!(h.syntax_for_path("Makefile").is_some());
        assert!(h.syntax_for_path("deep/dir/Makefile").is_some());
    }

    #[test]
    fn a_dotfile_is_not_treated_as_a_bare_extension() {
        // `Path::extension` returns None for ".rs", so the name fallback must
        // not accidentally match the Rust syntax on a file literally named
        // ".rs" -- but more importantly it must not panic.
        let h = highlighter();
        let _ = h.syntax_for_path(".rs");
        let _ = h.syntax_for_path(".gitignore");
    }

    // -- byte coverage -------------------------------------------------------

    #[test]
    fn ascii_lines_are_covered_exactly() {
        let lines = &[
            "fn main() {",
            "    let x = 1 + 2;",
            "    println!(\"{x}\");",
            "}",
        ];
        assert_exact_coverage(lines, &rust(lines));
    }

    #[test]
    fn multibyte_lines_are_covered_exactly() {
        let lines = &[
            "let s = \"héllo\";",
            "let e = \"🎉🎊\";",
            "// héllo 🎉 世界 — em dash",
            "let mixed = \"aé🎉b\";",
        ];
        let spans = rust(lines);
        assert_exact_coverage(lines, &spans);

        // Sanity-check that these really are multi-byte, so the test would
        // catch a char-vs-byte mixup rather than passing vacuously.
        assert!(lines.iter().any(|l| l.len() != l.chars().count()));
    }

    #[test]
    fn a_line_that_is_only_an_emoji_is_covered_exactly() {
        let lines = &["🎉"];
        let spans = rust(lines);
        assert_exact_coverage(lines, &spans);
        assert_eq!(spans[0].iter().map(|s| s.len).sum::<usize>(), 4);
    }

    #[test]
    fn empty_lines_yield_no_spans() {
        let lines = &["fn f() {", "", "}"];
        let spans = rust(lines);
        assert_exact_coverage(lines, &spans);
        assert!(spans[1].is_empty(), "an empty line needs no runs");
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let h = highlighter();
        let syntax = h.syntax_for_path("x.rs").expect("rust syntax exists");
        assert!(h.highlight_lines(syntax, &[]).is_empty());
    }

    #[test]
    fn whitespace_only_lines_are_covered_exactly() {
        let lines = &["    ", "\t\t", " \t "];
        assert_exact_coverage(lines, &rust(lines));
    }

    #[test]
    fn very_long_lines_are_covered_exactly() {
        let long = "x".repeat(10_000);
        let line = format!("let s = \"{long}\";");
        let lines = &[line.as_str()];
        assert_exact_coverage(lines, &rust(lines));
    }

    #[test]
    fn unbalanced_syntax_does_not_break_coverage() {
        // Deliberately broken Rust: unterminated string, stray braces.
        let lines = &[
            "let s = \"unterminated",
            "}}}}",
            "/* unterminated comment",
            "fn ??? (",
        ];
        assert_exact_coverage(lines, &rust(lines));
    }

    #[test]
    fn lines_are_highlighted_across_multiple_languages() {
        let h = highlighter();
        for (path, lines) in [
            ("a.py", vec!["def f(x):", "    return x + 1  # héllo"]),
            ("a.js", vec!["const x = `té${y}`;", "// 🎉"]),
            ("a.json", vec!["{\"k\": \"vé\"}", ""]),
            ("a.md", vec!["# Héading 🎉", "", "text"]),
        ] {
            let syntax = h.syntax_for_path(path).expect("syntax exists");
            let spans = h.highlight_lines(syntax, &lines);
            assert_exact_coverage(&lines, &spans);
        }
    }

    // -- degradation ---------------------------------------------------------

    #[test]
    fn unknown_language_degrades_to_one_unstyled_span_per_line() {
        let h = highlighter();
        let lines = &["some héllo 🎉 content", "another line", ""];
        let spans = h.highlight_lines_for_path("mystery.qqzz", lines);
        assert_exact_coverage(lines, &spans);
        assert_eq!(spans[0].len(), 1);
        assert_eq!(spans[0][0].style, h.plain_style());
        assert_eq!(spans[1].len(), 1);
        assert!(spans[2].is_empty());
    }

    #[test]
    fn highlight_lines_for_path_uses_the_syntax_when_it_is_known() {
        let h = highlighter();
        let lines = &["fn main() { let x: u32 = 1; }"];
        let spans = h.highlight_lines_for_path("src/main.rs", lines);
        assert_exact_coverage(lines, &spans);
        assert!(
            spans[0].len() > 1,
            "a known language should produce more than one run"
        );
        assert!(
            spans[0].iter().any(|s| s.style != h.plain_style()),
            "a known language should produce at least one styled run"
        );
    }

    #[test]
    fn unknown_language_with_no_lines_is_empty() {
        let h = highlighter();
        assert!(h.highlight_lines_for_path("mystery.qqzz", &[]).is_empty());
    }

    // -- style extraction ----------------------------------------------------

    #[test]
    fn the_dark_theme_produces_light_foregrounds() {
        let h = highlighter();
        let plain = h.plain_style();
        // base16-ocean.dark's foreground is a light grey; a light theme would
        // give a near-black value here.
        let brightness = u32::from(plain.r) + u32::from(plain.g) + u32::from(plain.b);
        assert!(brightness > 200, "expected a dark theme, got {plain:?}");
    }

    #[test]
    fn highlighter_is_debug_and_default() {
        let h = Highlighter::default();
        let rendered = format!("{h:?}");
        assert!(rendered.contains("Highlighter"), "{rendered}");
    }
}
