//! Unified-diff parsing for GitHub's per-file `patch` strings.
//!
//! `GET /repos/{owner}/{repo}/pulls/{number}/files` returns, per file, a patch
//! that starts directly at the first `@@` header — there is no `diff --git`
//! preamble and no `---`/`+++` file header:
//!
//! ```text
//! @@ -12,7 +12,9 @@ fn main() {
//!  context line
//! -removed line
//! +added line
//!  more context
//! ```
//!
//! The whole point of this module is [`walk`ing](parse_patch) each hunk from its
//! header's start offsets to recover per-line old/new line numbers, because
//! those numbers are what GitHub anchors review comments by.

use crate::model::{DiffLine, Hunk, LineKind};

/// Why a patch could not be parsed.
///
/// Every variant carries the 1-based line number *within the patch string* so a
/// malformed patch can be reported precisely.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiffParseError {
    /// A `@@` line was not of the form `@@ -old +new @@`.
    #[error("patch line {line_no}: malformed hunk header: {header:?}")]
    MalformedHunkHeader { line_no: usize, header: String },

    /// A hunk header's `-old` or `+new` range was not `start` or `start,count`.
    #[error("patch line {line_no}: malformed hunk range {range:?} in header {header:?}")]
    MalformedHunkRange {
        line_no: usize,
        header: String,
        range: String,
    },

    /// Content appeared before the first `@@` header. GitHub's per-file patches
    /// begin at a hunk header; anything else is a different diff format.
    #[error("patch line {line_no}: expected a hunk header, found {content:?}")]
    ExpectedHunkHeader { line_no: usize, content: String },

    /// A hunk body line did not start with ` `, `+`, `-`, or `\`.
    #[error("patch line {line_no}: unexpected line prefix in hunk body: {content:?}")]
    BadLinePrefix { line_no: usize, content: String },

    /// A `\ No newline at end of file` marker had no line to attach to.
    #[error("patch line {line_no}: `\\ No newline at end of file` with no preceding line")]
    DanglingNoNewlineMarker { line_no: usize },
}

/// Parse a GitHub per-file unified-diff patch into hunks with line numbers.
///
/// An empty patch yields an empty `Vec` rather than an error: GitHub omits the
/// `patch` field for binary and oversized files, and callers represent that with
/// [`PatchAvailability`](crate::model::PatchAvailability) instead.
///
/// Handled: multiple hunks, headers with omitted counts (`@@ -1 +1 @@` means a
/// count of 1), trailing function context after the closing `@@`,
/// `\ No newline at end of file` markers, trailing context after the last
/// change, and completely empty context lines emitted as a bare newline with no
/// leading space (real GitHub patches contain these).
///
/// # Errors
///
/// Returns [`DiffParseError`] if the patch is not a well-formed sequence of
/// hunks. The parser is deliberately strict: silently accepting a malformed
/// patch would produce plausible-looking but wrong line numbers, and wrong line
/// numbers put review comments on the wrong lines of someone else's pull
/// request.
pub fn parse_patch(patch: &str) -> Result<Vec<Hunk>, DiffParseError> {
    // A patch is a sequence of `\n`-terminated lines; the terminator on the
    // final line is optional. Stripping one trailing newline keeps `split` from
    // inventing a phantom empty line at the end — which would otherwise be
    // indistinguishable from a legitimately empty context line.
    let body = patch.strip_suffix('\n').unwrap_or(patch);
    if body.is_empty() {
        return Ok(Vec::new());
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;
    // Cursors track the next line number to hand out on each side. They are
    // reset from the header of every hunk, so a mistake cannot propagate past a
    // hunk boundary.
    let mut old_cursor: u32 = 0;
    let mut new_cursor: u32 = 0;

    for (idx, raw) in body.split('\n').enumerate() {
        let line_no = idx + 1;

        // Only a real header can start with `@@` at column 0: body lines always
        // carry a ` `, `+`, `-`, or `\` marker first.
        if raw.starts_with("@@") {
            if let Some(finished) = current.take() {
                hunks.push(check_counts(finished));
            }
            let header = parse_hunk_header(raw, line_no)?;
            old_cursor = header.old_start;
            new_cursor = header.new_start;
            current = Some(header);
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            return Err(DiffParseError::ExpectedHunkHeader {
                line_no,
                content: raw.to_owned(),
            });
        };

        // `\` is reserved by unified diff for the no-newline marker. It applies
        // to the line just emitted and is not itself a line of either file.
        if raw.starts_with('\\') {
            let last = hunk
                .lines
                .last_mut()
                .ok_or(DiffParseError::DanglingNoNewlineMarker { line_no })?;
            last.no_newline_at_eof = true;
            continue;
        }

        let (kind, content) = match raw.as_bytes().first() {
            Some(b' ') => (LineKind::Context, &raw[1..]),
            Some(b'+') => (LineKind::Added, &raw[1..]),
            Some(b'-') => (LineKind::Removed, &raw[1..]),
            // A completely empty line is an empty context line whose trailing
            // space was stripped. Git itself emits these, and so does GitHub.
            None => (LineKind::Context, ""),
            Some(_) => {
                return Err(DiffParseError::BadLinePrefix {
                    line_no,
                    content: raw.to_owned(),
                });
            }
        };

        // The walk: old advances on Context/Removed, new on Context/Added.
        // `number` maps a cursor of 0 to `None` because line 0 does not exist —
        // that only arises from a degenerate `-0,0`/`+0,0` header, and a `None`
        // makes the line non-commentable instead of anchoring at a bogus line.
        let (old_line, new_line) = match kind {
            LineKind::Context => {
                let pair = (number(old_cursor), number(new_cursor));
                old_cursor = old_cursor.saturating_add(1);
                new_cursor = new_cursor.saturating_add(1);
                pair
            }
            LineKind::Added => {
                let pair = (None, number(new_cursor));
                new_cursor = new_cursor.saturating_add(1);
                pair
            }
            LineKind::Removed => {
                let pair = (number(old_cursor), None);
                old_cursor = old_cursor.saturating_add(1);
                pair
            }
        };

        hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            content: content.to_owned(),
            no_newline_at_eof: false,
        });
    }

    if let Some(finished) = current.take() {
        hunks.push(check_counts(finished));
    }

    Ok(hunks)
}

/// Line numbers are 1-based; `0` means "no such line".
fn number(cursor: u32) -> Option<u32> {
    (cursor != 0).then_some(cursor)
}

/// Parse `@@ -old_start[,old_count] +new_start[,new_count] @@[ context]` into an
/// empty [`Hunk`] carrying the header's ranges.
fn parse_hunk_header(raw: &str, line_no: usize) -> Result<Hunk, DiffParseError> {
    let malformed = || DiffParseError::MalformedHunkHeader {
        line_no,
        header: raw.to_owned(),
    };

    let rest = raw.strip_prefix("@@ ").ok_or_else(malformed)?;
    // The ranges never contain " @@", so the first occurrence closes the header
    // and anything after it is function context we keep only inside `header`.
    let (ranges, _context) = rest.split_once(" @@").ok_or_else(malformed)?;
    let (old, new) = ranges.split_once(' ').ok_or_else(malformed)?;
    let old = old.strip_prefix('-').ok_or_else(malformed)?;
    let new = new.strip_prefix('+').ok_or_else(malformed)?;

    let (old_start, old_count) = parse_range(old, raw, line_no)?;
    let (new_start, new_count) = parse_range(new, raw, line_no)?;

    Ok(Hunk {
        header: raw.to_owned(),
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

/// Parse `start` or `start,count`. A missing count means 1.
fn parse_range(range: &str, header: &str, line_no: usize) -> Result<(u32, u32), DiffParseError> {
    let malformed = || DiffParseError::MalformedHunkRange {
        line_no,
        header: header.to_owned(),
        range: range.to_owned(),
    };

    match range.split_once(',') {
        Some((start, count)) => Ok((
            start.parse().map_err(|_| malformed())?,
            count.parse().map_err(|_| malformed())?,
        )),
        None => Ok((range.parse().map_err(|_| malformed())?, 1)),
    }
}

/// Log, but do not reject, a hunk whose body does not match its declared counts.
///
/// GitHub truncates oversized patches mid-hunk, and refusing to render a
/// truncated file is worse than rendering the part that arrived: the line
/// numbers of the lines we did get are still correct, because they come from the
/// walk rather than the counts.
fn check_counts(hunk: Hunk) -> Hunk {
    let old_seen = hunk
        .lines
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Context | LineKind::Removed))
        .count();
    let new_seen = hunk
        .lines
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Context | LineKind::Added))
        .count();

    if old_seen as u64 != u64::from(hunk.old_count) || new_seen as u64 != u64::from(hunk.new_count)
    {
        tracing::debug!(
            header = %hunk.header,
            declared_old = hunk.old_count,
            actual_old = old_seen,
            declared_new = hunk.new_count,
            actual_new = new_seen,
            "hunk body does not match its declared line counts; patch may be truncated"
        );
    }

    hunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CommentAnchor;
    use rostrum_core::Side;

    /// `(kind, old_line, new_line, content)` for every line of every hunk.
    fn flatten(hunks: &[Hunk]) -> Vec<(LineKind, Option<u32>, Option<u32>, &str)> {
        hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .map(|l| (l.kind, l.old_line, l.new_line, l.content.as_str()))
            .collect()
    }

    fn parse_ok(patch: &str) -> Vec<Hunk> {
        match parse_patch(patch) {
            Ok(hunks) => hunks,
            Err(err) => panic!("expected patch to parse, got {err}"),
        }
    }

    fn parse_err(patch: &str) -> DiffParseError {
        match parse_patch(patch) {
            Ok(hunks) => panic!("expected patch to fail, got {} hunks", hunks.len()),
            Err(err) => err,
        }
    }

    // -- empty / degenerate input --------------------------------------------

    #[test]
    fn empty_patch_is_empty_not_an_error() {
        assert_eq!(parse_ok(""), Vec::new());
        assert_eq!(parse_ok("\n"), Vec::new());
    }

    // -- headers -------------------------------------------------------------

    #[test]
    fn header_ranges_are_parsed() {
        let hunks = parse_ok("@@ -12,7 +34,9 @@\n context\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 12);
        assert_eq!(hunks[0].old_count, 7);
        assert_eq!(hunks[0].new_start, 34);
        assert_eq!(hunks[0].new_count, 9);
    }

    #[test]
    fn header_is_preserved_verbatim_including_function_context() {
        let hunks = parse_ok("@@ -12,7 +12,9 @@ fn main() {\n context\n");
        assert_eq!(hunks[0].header, "@@ -12,7 +12,9 @@ fn main() {");
    }

    #[test]
    fn function_context_containing_at_signs_does_not_confuse_the_header() {
        let hunks = parse_ok("@@ -1,1 +1,1 @@ fn f() { /* @@ */ }\n context\n");
        assert_eq!(hunks[0].header, "@@ -1,1 +1,1 @@ fn f() { /* @@ */ }");
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].new_start, 1);
    }

    #[test]
    fn omitted_counts_mean_one() {
        let hunks = parse_ok("@@ -5 +7 @@\n-a\n+b\n");
        assert_eq!(hunks[0].old_start, 5);
        assert_eq!(hunks[0].old_count, 1);
        assert_eq!(hunks[0].new_start, 7);
        assert_eq!(hunks[0].new_count, 1);
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Removed, Some(5), None, "a"),
                (LineKind::Added, None, Some(7), "b"),
            ]
        );
    }

    #[test]
    fn one_side_may_omit_its_count() {
        let hunks = parse_ok("@@ -5 +7,2 @@\n-a\n+b\n+c\n");
        assert_eq!((hunks[0].old_count, hunks[0].new_count), (1, 2));
    }

    #[test]
    fn zero_count_header_for_a_new_file() {
        let hunks = parse_ok("@@ -0,0 +1,2 @@\n+first\n+second\n");
        assert_eq!(hunks[0].old_start, 0);
        assert_eq!(hunks[0].old_count, 0);
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Added, None, Some(1), "first"),
                (LineKind::Added, None, Some(2), "second"),
            ]
        );
    }

    #[test]
    fn zero_count_header_for_a_deleted_file() {
        let hunks = parse_ok("@@ -1,2 +0,0 @@\n-first\n-second\n");
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Removed, Some(1), None, "first"),
                (LineKind::Removed, Some(2), None, "second"),
            ]
        );
    }

    #[test]
    fn a_line_numbered_zero_is_reported_as_absent_rather_than_line_zero() {
        // Degenerate input: a `-0,0` header with an old-side line in the body.
        // Anchoring this at "line 0" would be rejected by the API at best and
        // land somewhere arbitrary at worst, so it must be non-commentable.
        let hunks = parse_ok("@@ -0,0 +0,0 @@\n context\n");
        let line = &hunks[0].lines[0];
        assert_eq!(line.old_line, None);
        assert_eq!(line.new_line, None);
        assert_eq!(line.anchor("f"), None);
    }

    #[test]
    fn malformed_headers_are_rejected() {
        for bad in [
            "@@\n",
            "@@ -1,1 +1,1\n",
            "@@ -1,1 @@\n",
            "@@ 1,1 +1,1 @@\n",
            "@@ -1,1 1,1 @@\n",
            "@@-1,1 +1,1 @@\n",
        ] {
            let err = parse_err(bad);
            assert!(
                matches!(
                    err,
                    DiffParseError::MalformedHunkHeader { .. }
                        | DiffParseError::MalformedHunkRange { .. }
                ),
                "expected a header error for {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn non_numeric_ranges_are_rejected() {
        let err = parse_err("@@ -x,1 +1,1 @@\n");
        assert!(matches!(err, DiffParseError::MalformedHunkRange { .. }));

        let err = parse_err("@@ -1,y +1,1 @@\n");
        assert!(matches!(err, DiffParseError::MalformedHunkRange { .. }));

        let err = parse_err("@@ -1,1 +-2,1 @@\n");
        assert!(matches!(err, DiffParseError::MalformedHunkRange { .. }));
    }

    #[test]
    fn errors_report_the_patch_line_number() {
        let err = parse_err("@@ -1,1 +1,1 @@\n context\n@@ nope @@\n");
        assert_eq!(
            err,
            DiffParseError::MalformedHunkHeader {
                line_no: 3,
                header: "@@ nope @@".into(),
            }
        );
    }

    // -- body lines ----------------------------------------------------------

    #[test]
    fn markers_are_stripped_from_content() {
        let hunks = parse_ok("@@ -1,2 +1,2 @@\n ctx\n-old\n+new\n");
        let contents: Vec<&str> = hunks[0].lines.iter().map(|l| l.content.as_str()).collect();
        assert_eq!(contents, vec!["ctx", "old", "new"]);
    }

    #[test]
    fn only_the_first_marker_byte_is_stripped() {
        // A line of code that itself starts with `+`/`-`/space keeps that byte.
        let hunks = parse_ok("@@ -1,3 +1,3 @@\n  indented\n---\n++plus\n");
        let contents: Vec<&str> = hunks[0].lines.iter().map(|l| l.content.as_str()).collect();
        assert_eq!(contents, vec![" indented", "--", "+plus"]);
    }

    #[test]
    fn bare_empty_line_is_an_empty_context_line() {
        // Real GitHub patches drop the trailing space on blank context lines.
        let hunks = parse_ok("@@ -1,3 +1,3 @@\n one\n\n three\n");
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Context, Some(1), Some(1), "one"),
                (LineKind::Context, Some(2), Some(2), ""),
                (LineKind::Context, Some(3), Some(3), "three"),
            ]
        );
    }

    #[test]
    fn several_consecutive_blank_context_lines_all_advance_the_cursors() {
        let hunks = parse_ok("@@ -1,4 +1,4 @@\n\n\n\n end\n");
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Context, Some(1), Some(1), ""),
                (LineKind::Context, Some(2), Some(2), ""),
                (LineKind::Context, Some(3), Some(3), ""),
                (LineKind::Context, Some(4), Some(4), "end"),
            ]
        );
    }

    #[test]
    fn blank_added_and_removed_lines_keep_empty_content() {
        let hunks = parse_ok("@@ -1,1 +1,1 @@\n-\n+\n");
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Removed, Some(1), None, ""),
                (LineKind::Added, None, Some(1), ""),
            ]
        );
    }

    #[test]
    fn a_line_with_no_marker_is_rejected() {
        let err = parse_err("@@ -1,1 +1,1 @@\nno marker\n");
        assert_eq!(
            err,
            DiffParseError::BadLinePrefix {
                line_no: 2,
                content: "no marker".into(),
            }
        );
    }

    #[test]
    fn content_before_the_first_hunk_header_is_rejected() {
        let err = parse_err("diff --git a/x b/x\n@@ -1,1 +1,1 @@\n ctx\n");
        assert_eq!(
            err,
            DiffParseError::ExpectedHunkHeader {
                line_no: 1,
                content: "diff --git a/x b/x".into(),
            }
        );
    }

    #[test]
    fn carriage_returns_are_kept_as_content() {
        let hunks = parse_ok("@@ -1,1 +1,1 @@\n crlf\r\n");
        assert_eq!(hunks[0].lines[0].content, "crlf\r");
    }

    #[test]
    fn final_line_without_a_trailing_newline_is_still_parsed() {
        let hunks = parse_ok("@@ -1,2 +1,2 @@\n ctx\n+added");
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Context, Some(1), Some(1), "ctx"),
                (LineKind::Added, None, Some(2), "added"),
            ]
        );
    }

    #[test]
    fn multibyte_content_survives_marker_stripping() {
        let hunks = parse_ok("@@ -1,1 +1,1 @@\n+héllo 🎉 世界\n");
        assert_eq!(hunks[0].lines[0].content, "héllo 🎉 世界");
    }

    // -- no newline at eof ---------------------------------------------------

    #[test]
    fn no_newline_marker_attaches_to_the_preceding_line() {
        let hunks = parse_ok("@@ -1,1 +1,1 @@\n-old\n\\ No newline at end of file\n+new\n");
        assert_eq!(hunks[0].lines.len(), 2, "the marker is not itself a line");
        assert!(hunks[0].lines[0].no_newline_at_eof);
        assert!(!hunks[0].lines[1].no_newline_at_eof);
    }

    #[test]
    fn both_sides_may_carry_a_no_newline_marker() {
        let patch = "@@ -1,1 +1,1 @@\n\
                     -old\n\
                     \\ No newline at end of file\n\
                     +new\n\
                     \\ No newline at end of file\n";
        let hunks = parse_ok(patch);
        assert_eq!(hunks[0].lines.len(), 2);
        assert!(hunks[0].lines[0].no_newline_at_eof);
        assert!(hunks[0].lines[1].no_newline_at_eof);
    }

    #[test]
    fn no_newline_marker_does_not_advance_line_numbers() {
        let patch = "@@ -1,2 +1,2 @@\n ctx\n\\ No newline at end of file\n+added\n";
        let hunks = parse_ok(patch);
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Context, Some(1), Some(1), "ctx"),
                (LineKind::Added, None, Some(2), "added"),
            ]
        );
    }

    #[test]
    fn no_newline_marker_on_a_context_line_is_accepted() {
        let hunks = parse_ok("@@ -1,1 +1,1 @@\n ctx\n\\ No newline at end of file\n");
        assert!(hunks[0].lines[0].no_newline_at_eof);
    }

    #[test]
    fn dangling_no_newline_marker_is_rejected() {
        let err = parse_err("@@ -1,1 +1,1 @@\n\\ No newline at end of file\n");
        assert_eq!(err, DiffParseError::DanglingNoNewlineMarker { line_no: 2 });
    }

    // -- multiple hunks ------------------------------------------------------

    #[test]
    fn each_hunk_restarts_its_cursors_from_its_own_header() {
        let patch = "@@ -1,2 +1,2 @@\n ctx\n-old\n+new\n@@ -100,2 +200,2 @@\n ctx2\n-old2\n+new2\n";
        let hunks = parse_ok(patch);
        assert_eq!(hunks.len(), 2);
        assert_eq!(
            flatten(&hunks[..1]),
            vec![
                (LineKind::Context, Some(1), Some(1), "ctx"),
                (LineKind::Removed, Some(2), None, "old"),
                (LineKind::Added, None, Some(2), "new"),
            ]
        );
        assert_eq!(
            flatten(&hunks[1..]),
            vec![
                (LineKind::Context, Some(100), Some(200), "ctx2"),
                (LineKind::Removed, Some(101), None, "old2"),
                (LineKind::Added, None, Some(200 + 1), "new2"),
            ]
        );
    }

    #[test]
    fn drift_between_the_two_sides_accumulates_within_a_hunk() {
        // Three additions against one removal push the new-side numbers two
        // ahead of the old-side ones by the trailing context line.
        let patch = concat!(
            "@@ -12,7 +12,9 @@ fn main() {\n",
            " context line\n",
            "-removed line\n",
            "+added line\n",
            "+another added\n",
            "+third added\n",
            " more context\n",
        );
        let hunks = parse_ok(patch);
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Context, Some(12), Some(12), "context line"),
                (LineKind::Removed, Some(13), None, "removed line"),
                (LineKind::Added, None, Some(13), "added line"),
                (LineKind::Added, None, Some(14), "another added"),
                (LineKind::Added, None, Some(15), "third added"),
                (LineKind::Context, Some(14), Some(16), "more context"),
            ]
        );
    }

    #[test]
    fn trailing_context_after_the_last_change_is_kept() {
        let patch = concat!(
            "@@ -1,5 +1,5 @@\n",
            " a\n",
            "-b\n",
            "+B\n",
            " c\n",
            " d\n",
            " e\n",
        );
        let hunks = parse_ok(patch);
        let last = hunks[0].lines.last().expect("hunk has lines");
        assert_eq!(last.kind, LineKind::Context);
        assert_eq!(last.content, "e");
        assert_eq!((last.old_line, last.new_line), (Some(5), Some(5)));
    }

    #[test]
    fn three_hunks_all_parse() {
        let patch = concat!(
            "@@ -1 +1 @@\n",
            "-a\n",
            "+A\n",
            "@@ -10,2 +10,2 @@ mod x {\n",
            " ctx\n",
            "-b\n",
            "+B\n",
            "@@ -20,1 +20,2 @@\n",
            " ctx\n",
            "+c\n",
        );
        let hunks = parse_ok(patch);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].header, "@@ -1 +1 @@");
        assert_eq!(hunks[1].header, "@@ -10,2 +10,2 @@ mod x {");
        assert_eq!(hunks[2].header, "@@ -20,1 +20,2 @@");
        assert_eq!(
            hunks.iter().map(|h| h.lines.len()).collect::<Vec<_>>(),
            [2, 3, 2]
        );
    }

    // -- the anchoring invariant, end to end ---------------------------------

    #[test]
    fn every_line_of_a_multi_hunk_patch_anchors_correctly() {
        let patch = concat!(
            "@@ -10,4 +20,5 @@ fn first() {\n",
            " keep one\n",
            "-drop one\n",
            "+add one\n",
            "+add two\n",
            " keep two\n",
            " keep three\n",
            "@@ -50,3 +61,2 @@ fn second() {\n",
            " keep four\n",
            "-drop two\n",
            "-drop three\n",
            " keep five\n",
        );
        let hunks = parse_ok(patch);
        let anchors: Vec<Option<CommentAnchor>> = hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .map(|l| l.anchor("src/lib.rs"))
            .collect();

        let expect = |line: u32, side: Side| {
            Some(CommentAnchor {
                path: "src/lib.rs".into(),
                line,
                side,
            })
        };

        assert_eq!(
            anchors,
            vec![
                // hunk 1: old starts at 10, new at 20
                expect(20, Side::Right), // " keep one"   old 10 / new 20
                expect(11, Side::Left),  // "-drop one"   old 11
                expect(21, Side::Right), // "+add one"    new 21
                expect(22, Side::Right), // "+add two"    new 22
                expect(23, Side::Right), // " keep two"   old 12 / new 23
                expect(24, Side::Right), // " keep three" old 13 / new 24
                // hunk 2: old starts at 50, new at 61
                expect(61, Side::Right), // " keep four"  old 50 / new 61
                expect(51, Side::Left),  // "-drop two"   old 51
                expect(52, Side::Left),  // "-drop three" old 52
                expect(62, Side::Right), // " keep five"  old 53 / new 62
            ]
        );
    }

    #[test]
    fn context_lines_anchor_to_the_new_file_even_when_the_numbers_diverge() {
        // The single most dangerous confusion: a context line's old and new
        // numbers differ, and only the new one is a valid RIGHT-side anchor.
        let hunks = parse_ok("@@ -100,1 +7,1 @@\n ctx\n");
        let anchor = hunks[0].lines[0]
            .anchor("f")
            .expect("context lines are commentable");
        assert_eq!(anchor.side, Side::Right);
        assert_eq!(anchor.line, 7);
        assert_ne!(anchor.line, 100);
    }

    #[test]
    fn a_pure_addition_never_yields_a_left_anchor() {
        let hunks = parse_ok("@@ -0,0 +1,3 @@\n+a\n+b\n+c\n");
        for line in &hunks[0].lines {
            let anchor = line.anchor("f").expect("added lines are commentable");
            assert_eq!(anchor.side, Side::Right);
            assert_eq!(line.old_line, None);
        }
    }

    #[test]
    fn a_pure_deletion_never_yields_a_right_anchor() {
        let hunks = parse_ok("@@ -1,3 +0,0 @@\n-a\n-b\n-c\n");
        for line in &hunks[0].lines {
            let anchor = line.anchor("f").expect("removed lines are commentable");
            assert_eq!(anchor.side, Side::Left);
            assert_eq!(line.new_line, None);
        }
    }

    #[test]
    fn line_numbers_are_contiguous_and_monotonic_within_a_hunk() {
        let patch = concat!(
            "@@ -30,6 +40,7 @@\n",
            " a\n",
            "-b\n",
            "-c\n",
            "+B\n",
            "+C\n",
            "+D\n",
            " d\n",
            " e\n",
            " f\n",
        );
        let hunks = parse_ok(patch);
        let hunk = &hunks[0];

        let mut expected_old = hunk.old_start;
        let mut expected_new = hunk.new_start;
        for line in &hunk.lines {
            match line.kind {
                LineKind::Context => {
                    assert_eq!(line.old_line, Some(expected_old));
                    assert_eq!(line.new_line, Some(expected_new));
                    expected_old += 1;
                    expected_new += 1;
                }
                LineKind::Removed => {
                    assert_eq!(line.old_line, Some(expected_old));
                    assert_eq!(line.new_line, None);
                    expected_old += 1;
                }
                LineKind::Added => {
                    assert_eq!(line.old_line, None);
                    assert_eq!(line.new_line, Some(expected_new));
                    expected_new += 1;
                }
            }
        }
        // The walk consumed exactly as many lines as the header declared.
        assert_eq!(expected_old - hunk.old_start, hunk.old_count);
        assert_eq!(expected_new - hunk.new_start, hunk.new_count);
    }

    #[test]
    fn a_truncated_hunk_still_yields_correct_numbers_for_the_lines_present() {
        // Header claims 9 old lines; only 2 arrived. Parsing must not fail, and
        // the numbers that did arrive must still be right.
        let hunks = parse_ok("@@ -5,9 +5,9 @@\n a\n-b\n");
        assert_eq!(
            flatten(&hunks),
            vec![
                (LineKind::Context, Some(5), Some(5), "a"),
                (LineKind::Removed, Some(6), None, "b"),
            ]
        );
    }
}
