//! Checking an anchor Kotlin hands back against the diff it came from.
//!
//! Anchors are computed in Rust only: every commentable line of `file_diff`
//! carries the anchor `DiffLine::anchor` derived from that very line. Kotlin
//! passes one back to `add_draft`, and this confirms it is still a real,
//! commentable line of the current head's diff — so a stale or hand-built
//! anchor is refused rather than landing a comment on the wrong line of
//! someone else's pull request.

use rostrum_core::{DraftAnchor as CoreAnchor, Side as CoreSide};
use rostrum_diff::DiffFile;

use crate::{diff::CommentAnchor, error::RostrumError};

/// The draft anchor for a comment on `end`, or on `start..=end` when a range
/// start is given. Both must be commentable lines of the same hunk, on the
/// same side of the same file; the range comes back ordered.
pub(crate) fn resolve(
    files: &[DiffFile],
    end: &CommentAnchor,
    start: Option<&CommentAnchor>,
) -> Result<CoreAnchor, RostrumError> {
    let side = CoreSide::from(end.side);
    let file = files
        .iter()
        .find(|file| file.path == end.path)
        .ok_or_else(|| RostrumError::invalid(format!("{} is not in this diff", end.path)))?;
    let end_hunk = hunk_of(file, end.line, side).ok_or_else(|| missing(end))?;
    let Some(start) = start else {
        return Ok(CoreAnchor::single(&file.path, end.line, side));
    };
    if start.path != end.path {
        return Err(RostrumError::invalid("a comment range must stay within one file"));
    }
    if start.side != end.side {
        return Err(RostrumError::invalid(
            "a comment range must stay on one side of the diff",
        ));
    }
    let start_hunk = hunk_of(file, start.line, side).ok_or_else(|| missing(start))?;
    if start_hunk != end_hunk {
        return Err(RostrumError::invalid("a comment range must stay within one hunk"));
    }
    Ok(CoreAnchor::single(&file.path, start.line, side).extended_to(end.line, side))
}

/// The hunk holding a commentable line anchored at `line` on `side`.
fn hunk_of(file: &DiffFile, line: u32, side: CoreSide) -> Option<usize> {
    file.hunks.iter().position(|hunk| {
        hunk.lines.iter().any(|diff_line| {
            diff_line
                .anchor(&file.path)
                .is_some_and(|anchor| anchor.line == line && anchor.side == side)
        })
    })
}

fn missing(anchor: &CommentAnchor) -> RostrumError {
    let side = match anchor.side {
        crate::types::Side::Left => "old",
        crate::types::Side::Right => "new",
    };
    RostrumError::invalid(format!(
        "line {} of the {side} {} is not a commentable line of the current diff",
        anchor.line, anchor.path
    ))
}

#[cfg(test)]
mod tests {
    use rostrum_diff::{FileStatus, parse_patch};

    use super::*;
    use crate::types::Side;

    /// Two hunks: lines 10–12 (old) / 10–12 (new) with a replacement, and a
    /// later hunk adding line 40.
    fn files() -> Vec<DiffFile> {
        let patch = "@@ -10,3 +10,3 @@ fn a() {\n context\n-old\n+new\n context\n@@ -39,1 +39,2 @@\n tail\n+added\n";
        vec![DiffFile {
            path: "src/lib.rs".into(),
            previous_path: None,
            status: FileStatus::Modified,
            additions: 2,
            deletions: 1,
            hunks: parse_patch(patch).expect("patch"),
            availability: rostrum_diff::PatchAvailability::Present,
        }]
    }

    fn at(line: u32, side: Side) -> CommentAnchor {
        CommentAnchor {
            path: "src/lib.rs".into(),
            line,
            side,
        }
    }

    #[test]
    fn a_line_in_the_diff_resolves_to_a_single_anchor() {
        let anchor = resolve(&files(), &at(11, Side::Right), None).expect("valid");
        assert_eq!(anchor, CoreAnchor::single("src/lib.rs", 11, CoreSide::Right));
        // The removed line's old number, on the left.
        let removed = resolve(&files(), &at(11, Side::Left), None).expect("valid");
        assert_eq!(removed.side, CoreSide::Left);
    }

    #[test]
    fn lines_outside_the_diff_are_refused() {
        for bad in [
            at(13, Side::Right),
            at(40, Side::Left),
            at(1, Side::Right),
            CommentAnchor {
                path: "src/other.rs".into(),
                line: 11,
                side: Side::Right,
            },
        ] {
            assert!(
                matches!(resolve(&files(), &bad, None), Err(RostrumError::InvalidInput { .. })),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_range_is_ordered_and_single_sided() {
        let range = resolve(&files(), &at(10, Side::Right), Some(&at(12, Side::Right)))
            .expect("valid range");
        assert_eq!((range.start_line, range.line), (Some(10), 12));
        assert_eq!(range.start_side, Some(CoreSide::Right));

        let same = resolve(&files(), &at(11, Side::Right), Some(&at(11, Side::Right)))
            .expect("valid");
        assert_eq!(same.start_line, None);
    }

    #[test]
    fn a_range_across_sides_hunks_or_files_is_refused() {
        let across_sides = resolve(&files(), &at(12, Side::Right), Some(&at(11, Side::Left)));
        assert!(matches!(across_sides, Err(RostrumError::InvalidInput { .. })));

        let across_hunks = resolve(&files(), &at(40, Side::Right), Some(&at(10, Side::Right)));
        assert!(matches!(across_hunks, Err(RostrumError::InvalidInput { .. })));

        let across_files = resolve(
            &files(),
            &at(11, Side::Right),
            Some(&CommentAnchor {
                path: "src/other.rs".into(),
                line: 10,
                side: Side::Right,
            }),
        );
        assert!(matches!(across_files, Err(RostrumError::InvalidInput { .. })));
    }
}
