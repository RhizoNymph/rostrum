//! Word-level changes inside a modified line.
//!
//! A diff marks whole lines as removed and added; when one line was edited
//! rather than replaced, the reader wants the few words that changed picked
//! out. This pairs each run of removed lines with the run of added lines
//! directly after it (first with first, second with second — the way GitHub
//! pairs them) and finds the changed words of each pair with a longest-common-
//! subsequence over tokens.
//!
//! Ranges are **UTF-8 byte ranges** into [`DiffLine::content`], always on
//! `char` boundaries. Best-effort by design: pairs too long to compare
//! cheaply, and pairs with too little in common for emphasis to mean
//! anything, get no emphasis rather than a wall of it.

use std::ops::Range;

use crate::model::{DiffLine, LineKind};

/// Token-pair budget for one comparison. The table is `old × new` tokens;
/// beyond this a line pair is left unemphasised.
const MAX_CELLS: usize = 64 * 1024;

/// The changed words of one removed/added pair.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WordChanges {
    /// Ranges in the removed line.
    pub old: Vec<Range<usize>>,
    /// Ranges in the added line.
    pub new: Vec<Range<usize>>,
}

/// The changed words between `old` and `new`.
///
/// Empty when the lines are identical, when they share less than half their
/// non-whitespace text (emphasising most of both lines says nothing the line
/// colours do not), or when they are too long to compare cheaply.
pub fn word_changes(old: &str, new: &str) -> WordChanges {
    if old == new {
        return WordChanges::default();
    }
    let old_tokens = tokenize(old);
    let new_tokens = tokenize(new);
    if old_tokens.len().saturating_mul(new_tokens.len()) > MAX_CELLS {
        return WordChanges::default();
    }

    let (old_kept, new_kept) = common_tokens(old, &old_tokens, new, &new_tokens);

    let shared: usize = old_tokens
        .iter()
        .zip(&old_kept)
        .filter(|(token, kept)| **kept && !is_blank(&old[(*token).clone()]))
        .map(|(token, _)| token.len())
        .sum();
    let substance = non_blank_len(old).max(non_blank_len(new));
    if shared == 0 || shared * 2 < substance {
        return WordChanges::default();
    }

    WordChanges {
        old: changed_ranges(old, &old_tokens, &old_kept),
        new: changed_ranges(new, &new_tokens, &new_kept),
    }
}

/// Emphasis for every line of a hunk, one entry per line: empty for context
/// lines and for removed or added lines with no counterpart.
pub fn hunk_word_changes(lines: &[DiffLine]) -> Vec<Vec<Range<usize>>> {
    let mut emphasis = vec![Vec::new(); lines.len()];
    let mut ix = 0;
    while ix < lines.len() {
        if lines[ix].kind != LineKind::Removed {
            ix += 1;
            continue;
        }
        let removed_start = ix;
        while ix < lines.len() && lines[ix].kind == LineKind::Removed {
            ix += 1;
        }
        let added_start = ix;
        while ix < lines.len() && lines[ix].kind == LineKind::Added {
            ix += 1;
        }
        let pairs = (added_start - removed_start).min(ix - added_start);
        for offset in 0..pairs {
            let (old_ix, new_ix) = (removed_start + offset, added_start + offset);
            let changes = word_changes(&lines[old_ix].content, &lines[new_ix].content);
            emphasis[old_ix] = changes.old;
            emphasis[new_ix] = changes.new;
        }
    }
    emphasis
}

/// Split into tokens: runs of word characters, runs of whitespace, and single
/// other characters. Returned as byte ranges covering the whole string.
fn tokenize(text: &str) -> Vec<Range<usize>> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };

    let mut tokens: Vec<Range<usize>> = Vec::new();
    let mut current: Option<(Class, usize)> = None;
    for (at, c) in text.char_indices() {
        let kind = class(c);
        match current {
            Some((open, _)) if open == kind && kind != Class::Other => {}
            Some((_, start)) => {
                tokens.push(start..at);
                current = Some((kind, at));
            }
            None => current = Some((kind, at)),
        }
    }
    if let Some((_, start)) = current {
        tokens.push(start..text.len());
    }
    tokens
}

/// Mark which tokens of each side belong to a longest common subsequence.
fn common_tokens(
    old: &str,
    old_tokens: &[Range<usize>],
    new: &str,
    new_tokens: &[Range<usize>],
) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (old_tokens.len(), new_tokens.len());
    let same = |i: usize, j: usize| old[old_tokens[i].clone()] == new[new_tokens[j].clone()];

    // lengths[i][j]: LCS length of old_tokens[i..] and new_tokens[j..].
    let width = m + 1;
    let mut lengths = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lengths[i * width + j] = if same(i, j) {
                lengths[(i + 1) * width + j + 1] + 1
            } else {
                lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
            };
        }
    }

    let mut old_kept = vec![false; n];
    let mut new_kept = vec![false; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(i, j) {
            old_kept[i] = true;
            new_kept[j] = true;
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * width + j] >= lengths[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (old_kept, new_kept)
}

/// Coalesce the tokens that are not kept into ranges. Whitespace kept between
/// two changed tokens is folded in, so `a b` → `c d` emphasises one run rather
/// than two words with a gap.
fn changed_ranges(text: &str, tokens: &[Range<usize>], kept: &[bool]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut ix = 0;
    while ix < tokens.len() {
        if kept[ix] {
            ix += 1;
            continue;
        }
        let start = tokens[ix].start;
        let mut end = tokens[ix].end;
        ix += 1;
        loop {
            if ix < tokens.len() && !kept[ix] {
                end = tokens[ix].end;
                ix += 1;
            } else if ix + 1 < tokens.len()
                && is_blank(&text[tokens[ix].clone()])
                && !kept[ix + 1]
            {
                end = tokens[ix + 1].end;
                ix += 2;
            } else {
                break;
            }
        }
        ranges.push(start..end);
    }
    ranges
}

fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn non_blank_len(text: &str) -> usize {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(char::len_utf8)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slices<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|range| &text[range.clone()]).collect()
    }

    fn line(kind: LineKind, content: &str) -> DiffLine {
        DiffLine {
            kind,
            old_line: None,
            new_line: None,
            content: content.into(),
            no_newline_at_eof: false,
        }
    }

    #[test]
    fn tokens_cover_the_whole_line() {
        let text = "let x_1 = foo(a, b);  // ünïcode";
        let tokens = tokenize(text);
        let joined: String = tokens.iter().map(|t| &text[t.clone()]).collect();
        assert_eq!(joined, text);
        assert_eq!(
            slices(text, &tokens[..6]),
            vec!["let", " ", "x_1", " ", "=", " "]
        );
        assert!(slices(text, &tokens).contains(&"ünïcode"));
    }

    #[test]
    fn a_changed_argument_is_picked_out_on_both_sides() {
        let changes = word_changes("let x = compute(a, b);", "let x = compute(a, c);");
        assert_eq!(slices("let x = compute(a, b);", &changes.old), vec!["b"]);
        assert_eq!(slices("let x = compute(a, c);", &changes.new), vec!["c"]);
    }

    #[test]
    fn an_insertion_emphasises_only_the_new_side() {
        let old = "call(a, b)";
        let new = "call(a, extra, b)";
        let changes = word_changes(old, new);
        assert!(changes.old.is_empty(), "{:?}", slices(old, &changes.old));
        assert_eq!(slices(new, &changes.new), vec!["extra, "]);
    }

    #[test]
    fn adjacent_changed_words_coalesce_across_whitespace() {
        let old = "the quick brown fox jumps";
        let new = "the slow red fox jumps";
        let changes = word_changes(old, new);
        assert_eq!(slices(old, &changes.old), vec!["quick brown"]);
        assert_eq!(slices(new, &changes.new), vec!["slow red"]);
    }

    #[test]
    fn identical_lines_have_no_emphasis() {
        assert_eq!(word_changes("same", "same"), WordChanges::default());
    }

    #[test]
    fn lines_with_little_in_common_have_no_emphasis() {
        let changes = word_changes("return value;", "let other = compute_something(x, y);");
        assert_eq!(changes, WordChanges::default());
    }

    #[test]
    fn a_whitespace_only_change_is_emphasised() {
        let old = "a  b";
        let new = "a b";
        let changes = word_changes(old, new);
        assert_eq!(slices(old, &changes.old), vec!["  "]);
        assert_eq!(slices(new, &changes.new), vec![" "]);
    }

    #[test]
    fn ranges_fall_on_char_boundaries() {
        let old = "naïve café";
        let new = "naïve cafés";
        let changes = word_changes(old, new);
        for range in &changes.old {
            assert!(old.is_char_boundary(range.start) && old.is_char_boundary(range.end));
        }
        for range in &changes.new {
            assert!(new.is_char_boundary(range.start) && new.is_char_boundary(range.end));
        }
        assert_eq!(slices(new, &changes.new), vec!["cafés"]);
    }

    #[test]
    fn very_long_lines_are_left_alone() {
        let old = "a ".repeat(400);
        let new = format!("{}b", "a ".repeat(400));
        assert_eq!(word_changes(&old, &new), WordChanges::default());
    }

    #[test]
    fn hunk_pairs_removed_runs_with_the_added_runs_after_them() {
        let lines = vec![
            line(LineKind::Context, "fn main() {"),
            line(LineKind::Removed, "    let a = 1;"),
            line(LineKind::Removed, "    let b = 2;"),
            line(LineKind::Added, "    let a = 10;"),
            line(LineKind::Context, "}"),
            line(LineKind::Added, "// trailing"),
        ];
        let emphasis = hunk_word_changes(&lines);
        assert_eq!(emphasis.len(), lines.len());
        assert!(emphasis[0].is_empty());
        assert_eq!(slices(&lines[1].content, &emphasis[1]), vec!["1"]);
        // The second removal has no added counterpart.
        assert!(emphasis[2].is_empty());
        assert_eq!(slices(&lines[3].content, &emphasis[3]), vec!["10"]);
        assert!(emphasis[4].is_empty());
        // An addition not preceded by removals is not paired.
        assert!(emphasis[5].is_empty());
    }

    #[test]
    fn an_added_run_before_a_removed_run_is_not_paired() {
        let lines = vec![
            line(LineKind::Added, "x = 1"),
            line(LineKind::Removed, "x = 2"),
        ];
        let emphasis = hunk_word_changes(&lines);
        assert!(emphasis.iter().all(Vec::is_empty));
    }
}
