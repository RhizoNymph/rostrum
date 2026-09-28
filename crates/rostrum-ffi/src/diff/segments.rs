//! Syntax runs and word-level emphasis merged into styled segments.
//!
//! The highlighter describes a line as byte-length runs; word diff describes
//! it as byte ranges. Kotlin wants neither — Kotlin strings index by UTF-16
//! unit — so the two are cut together into segments that each carry their
//! own text. Concatenated, a line's segments are exactly the line.

use std::ops::Range;

use rostrum_diff::{HighlightSpan, SpanStyle};

use crate::diff::CodeSegment;

/// Opaque ARGB for a syntax colour.
pub(crate) fn argb(style: SpanStyle) -> u32 {
    0xFF00_0000 | (u32::from(style.r) << 16) | (u32::from(style.g) << 8) | u32::from(style.b)
}

/// Cut `text` at every highlight and emphasis boundary. Spans that do not
/// exactly cover the line (a highlighter bug) degrade to one `plain` run
/// rather than misplacing colours; emphasis off a char boundary is ignored.
pub(crate) fn segments(
    text: &str,
    spans: &[HighlightSpan],
    emphasis: &[Range<usize>],
    plain: SpanStyle,
) -> Vec<CodeSegment> {
    if text.is_empty() {
        return Vec::new();
    }
    let covered: usize = spans.iter().map(|span| span.len).sum();
    let runs: Vec<(Range<usize>, SpanStyle)> = if covered == text.len() && !spans.is_empty() {
        let mut at = 0;
        spans
            .iter()
            .filter(|span| span.len > 0)
            .map(|span| {
                let run = at..at + span.len;
                at += span.len;
                (run, span.style)
            })
            .collect()
    } else {
        vec![(0..text.len(), plain)]
    };
    let emphasis: Vec<Range<usize>> = emphasis
        .iter()
        .filter(|range| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
        .cloned()
        .collect();

    let mut cuts: Vec<usize> = runs
        .iter()
        .flat_map(|(run, _)| [run.start, run.end])
        .chain(emphasis.iter().flat_map(|range| [range.start, range.end]))
        .filter(|&cut| text.is_char_boundary(cut))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();

    let mut out: Vec<CodeSegment> = Vec::new();
    for window in cuts.windows(2) {
        let (start, end) = (window[0], window[1]);
        let style = runs
            .iter()
            .find(|(run, _)| run.contains(&start))
            .map_or(plain, |(_, style)| *style);
        let emphasized = emphasis.iter().any(|range| range.contains(&start));
        let piece = &text[start..end];
        match out.last_mut() {
            Some(last)
                if last.color == argb(style)
                    && last.bold == style.bold
                    && last.italic == style.italic
                    && last.emphasized == emphasized =>
            {
                last.text.push_str(piece);
            }
            _ => out.push(CodeSegment {
                text: piece.to_string(),
                color: argb(style),
                bold: style.bold,
                italic: style.italic,
                emphasized,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: SpanStyle = SpanStyle {
        r: 0xc0,
        g: 0xc5,
        b: 0xce,
        bold: false,
        italic: false,
    };
    const KEYWORD: SpanStyle = SpanStyle {
        r: 0xb4,
        g: 0x8e,
        b: 0xad,
        bold: true,
        italic: false,
    };

    fn span(len: usize, style: SpanStyle) -> HighlightSpan {
        HighlightSpan { len, style }
    }

    fn joined(segments: &[CodeSegment]) -> String {
        segments.iter().map(|segment| segment.text.as_str()).collect()
    }

    #[test]
    fn colours_are_opaque_argb() {
        assert_eq!(argb(KEYWORD), 0xFFB4_8EAD);
    }

    #[test]
    fn runs_become_segments_that_rebuild_the_line() {
        let text = "let x = 1;";
        let out = segments(text, &[span(3, KEYWORD), span(7, PLAIN)], &[], PLAIN);
        assert_eq!(joined(&out), text);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "let");
        assert!(out[0].bold);
        assert_eq!(out[1].color, argb(PLAIN));
    }

    #[test]
    fn emphasis_cuts_through_runs() {
        let text = "let x = 10;";
        // "10" is bytes 8..10, inside the plain run.
        let emphasis = vec![Range { start: 8, end: 10 }];
        let out = segments(text, &[span(3, KEYWORD), span(8, PLAIN)], &emphasis, PLAIN);
        assert_eq!(joined(&out), text);
        let emphasized: Vec<&str> = out
            .iter()
            .filter(|segment| segment.emphasized)
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(emphasized, vec!["10"]);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn identical_neighbours_merge() {
        let out = segments("abcd", &[span(2, PLAIN), span(2, PLAIN)], &[], PLAIN);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "abcd");
    }

    #[test]
    fn spans_that_miss_the_line_degrade_to_plain() {
        let out = segments("abcdef", &[span(2, KEYWORD)], &[], PLAIN);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].color, argb(PLAIN));
        assert_eq!(joined(&out), "abcdef");
    }

    #[test]
    fn multibyte_text_is_cut_only_on_char_boundaries() {
        let text = "naïve café";
        let len = text.len();
        // Emphasis starting mid-char is ignored; one on a boundary applies.
        let out = segments(text, &[span(len, PLAIN)], &[3..4, 7..len], PLAIN);
        assert_eq!(joined(&out), text);
        assert!(out.iter().any(|segment| segment.emphasized && segment.text == "café"));
    }

    #[test]
    fn an_empty_line_has_no_segments() {
        assert!(segments("", &[], &[], PLAIN).is_empty());
    }
}
