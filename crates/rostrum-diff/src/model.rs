//! The diff row model: files, hunks, lines, and the comment anchors derived
//! from them.
//!
//! The types here are deliberately dumb data. The one piece of logic that lives
//! in this module is [`DiffLine::anchor`], because the side/line-number pairing
//! is the invariant the whole review surface rests on and it must have exactly
//! one implementation.

use rostrum_core::Side;

/// What a line in a hunk does to the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// Present unchanged in both the old and the new file.
    Context,
    /// Present only in the new file.
    Added,
    /// Present only in the old file.
    Removed,
}

impl LineKind {
    /// The diff side a review comment on this kind of line must be anchored to.
    ///
    /// GitHub's review-comment API anchors by `path` + `line` + `side`, where
    /// `RIGHT` addresses the *new* file and `LEFT` the *old* one. [`Added`] and
    /// [`Context`] lines exist in the new file, so they anchor to
    /// [`Side::Right`]; [`Removed`] lines only exist in the old file, so they
    /// anchor to [`Side::Left`].
    ///
    /// [`Added`]: LineKind::Added
    /// [`Context`]: LineKind::Context
    /// [`Removed`]: LineKind::Removed
    pub fn anchor_side(self) -> Side {
        match self {
            Self::Added | Self::Context => Side::Right,
            Self::Removed => Side::Left,
        }
    }
}

/// A single line of a hunk, with its reconstructed line numbers.
///
/// Exactly one of `old_line`/`new_line` is `None` for [`LineKind::Added`] and
/// [`LineKind::Removed`]; both are populated for [`LineKind::Context`]. (The
/// sole exception is a malformed hunk whose header start offset is `0`, where
/// the corresponding number is `None` because line `0` is not addressable.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based line number in the old file. `None` for added lines.
    pub old_line: Option<u32>,
    /// 1-based line number in the new file. `None` for removed lines.
    pub new_line: Option<u32>,
    /// The line's text with the leading `+`/`-`/space marker stripped, and
    /// without a trailing newline.
    pub content: String,
    /// The patch carried a `\ No newline at end of file` marker for this line.
    pub no_newline_at_eof: bool,
}

impl DiffLine {
    /// The anchor GitHub's review-comment API needs for this line, or `None` if
    /// the line cannot be commented on.
    ///
    /// The side and the line number are chosen by the same `match`, so they can
    /// never disagree: an [`Added`]/[`Context`] line yields `new_line` +
    /// [`Side::Right`], a [`Removed`] line yields `old_line` + [`Side::Left`].
    /// If the relevant number is missing the line is not commentable and the UI
    /// must disable the affordance rather than guess.
    ///
    /// [`Added`]: LineKind::Added
    /// [`Context`]: LineKind::Context
    /// [`Removed`]: LineKind::Removed
    pub fn anchor(&self, path: &str) -> Option<CommentAnchor> {
        let side = self.kind.anchor_side();
        let line = match side {
            Side::Right => self.new_line?,
            Side::Left => self.old_line?,
        };
        Some(CommentAnchor {
            path: path.to_owned(),
            line,
            side,
        })
    }

    /// Whether [`anchor`](Self::anchor) would return a value.
    pub fn is_commentable(&self) -> bool {
        match self.kind.anchor_side() {
            Side::Right => self.new_line.is_some(),
            Side::Left => self.old_line.is_some(),
        }
    }
}

/// Where a review comment attaches in a pull request's diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentAnchor {
    pub path: String,
    /// 1-based line number within the file identified by `side`.
    pub line: u32,
    pub side: Side,
}

/// One `@@ ... @@` section of a unified diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The raw `@@ ... @@` line, including any trailing function context, so
    /// the UI can render it verbatim.
    pub header: String,
    /// 1-based first line of the hunk in the old file (`0` when the old file is
    /// empty).
    pub old_start: u32,
    /// Number of old-file lines the hunk covers, as declared by the header.
    pub old_count: u32,
    /// 1-based first line of the hunk in the new file (`0` when the new file is
    /// empty).
    pub new_start: u32,
    /// Number of new-file lines the hunk covers, as declared by the header.
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

/// A file's status in a pull request, as reported by GitHub's files endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileStatus {
    Added,
    Removed,
    Modified,
    Renamed,
    Copied,
    Changed,
    Unchanged,
}

impl FileStatus {
    /// Parse GitHub's lowercase `status` string.
    ///
    /// Unrecognised values degrade to [`FileStatus::Modified`] with a warning
    /// rather than failing the whole file listing.
    pub fn from_api(s: &str) -> Self {
        match s {
            "added" => Self::Added,
            "removed" => Self::Removed,
            "modified" => Self::Modified,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            "changed" => Self::Changed,
            "unchanged" => Self::Unchanged,
            other => {
                tracing::warn!(status = other, "unknown file status; treating as modified");
                Self::Modified
            }
        }
    }
}

/// Whether a file's patch text is usable.
///
/// GitHub omits `patch` for binary files and for files whose diff exceeds its
/// size limits, and truncates very large diffs. The UI renders a placeholder
/// row instead of an empty file in those cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatchAvailability {
    /// The patch was supplied in full and parsed.
    Present,
    /// GitHub supplied no patch at all (binary, or too large).
    Omitted,
    /// GitHub supplied a patch it marked as truncated.
    Truncated,
}

/// One file's worth of a pull request diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFile {
    /// Path in the new tree (or the old tree for a removed file).
    pub path: String,
    /// Path in the old tree, set only for renames and copies.
    pub previous_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<Hunk>,
    pub availability: PatchAvailability,
}

impl DiffFile {
    /// Every line of every hunk, in patch order.
    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> {
        self.hunks.iter().flat_map(|hunk| hunk.lines.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            old_line: old,
            new_line: new,
            content: String::new(),
            no_newline_at_eof: false,
        }
    }

    #[test]
    fn added_line_anchors_right_on_new_line() {
        let l = line(LineKind::Added, None, Some(42));
        assert_eq!(
            l.anchor("src/main.rs"),
            Some(CommentAnchor {
                path: "src/main.rs".into(),
                line: 42,
                side: Side::Right,
            })
        );
    }

    #[test]
    fn context_line_anchors_right_on_new_line_not_old() {
        // The old and new numbers differ, so an implementation that used the
        // wrong one would land the comment on the wrong line.
        let l = line(LineKind::Context, Some(10), Some(17));
        let anchor = l.anchor("f").expect("context lines are commentable");
        assert_eq!(anchor.side, Side::Right);
        assert_eq!(anchor.line, 17);
    }

    #[test]
    fn removed_line_anchors_left_on_old_line() {
        let l = line(LineKind::Removed, Some(9), None);
        assert_eq!(
            l.anchor("src/main.rs"),
            Some(CommentAnchor {
                path: "src/main.rs".into(),
                line: 9,
                side: Side::Left,
            })
        );
    }

    #[test]
    fn line_without_its_anchor_number_is_not_commentable() {
        for l in [
            line(LineKind::Added, Some(3), None),
            line(LineKind::Context, Some(3), None),
            line(LineKind::Removed, None, Some(3)),
        ] {
            assert_eq!(l.anchor("f"), None);
            assert!(!l.is_commentable());
        }
    }

    #[test]
    fn is_commentable_agrees_with_anchor() {
        for kind in [LineKind::Context, LineKind::Added, LineKind::Removed] {
            for old in [None, Some(1)] {
                for new in [None, Some(2)] {
                    let l = line(kind, old, new);
                    assert_eq!(l.is_commentable(), l.anchor("f").is_some());
                }
            }
        }
    }

    #[test]
    fn anchor_side_matches_github_semantics() {
        assert_eq!(LineKind::Added.anchor_side(), Side::Right);
        assert_eq!(LineKind::Context.anchor_side(), Side::Right);
        assert_eq!(LineKind::Removed.anchor_side(), Side::Left);
        assert_eq!(Side::Right.as_api_str(), "RIGHT");
        assert_eq!(Side::Left.as_api_str(), "LEFT");
    }

    #[test]
    fn anchor_carries_the_path_it_was_given() {
        let l = line(LineKind::Added, None, Some(1));
        let anchor = l.anchor("crates/a/src/b.rs").expect("commentable");
        assert_eq!(anchor.path, "crates/a/src/b.rs");
    }

    #[test]
    fn file_status_from_api_covers_every_github_value() {
        assert_eq!(FileStatus::from_api("added"), FileStatus::Added);
        assert_eq!(FileStatus::from_api("removed"), FileStatus::Removed);
        assert_eq!(FileStatus::from_api("modified"), FileStatus::Modified);
        assert_eq!(FileStatus::from_api("renamed"), FileStatus::Renamed);
        assert_eq!(FileStatus::from_api("copied"), FileStatus::Copied);
        assert_eq!(FileStatus::from_api("changed"), FileStatus::Changed);
        assert_eq!(FileStatus::from_api("unchanged"), FileStatus::Unchanged);
    }

    #[test]
    fn file_status_from_api_degrades_on_unknown_values() {
        assert_eq!(FileStatus::from_api("Added"), FileStatus::Modified);
        assert_eq!(FileStatus::from_api(""), FileStatus::Modified);
        assert_eq!(FileStatus::from_api("teleported"), FileStatus::Modified);
    }

    #[test]
    fn diff_file_lines_walks_every_hunk() {
        let file = DiffFile {
            path: "a.rs".into(),
            previous_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            hunks: vec![
                Hunk {
                    header: "@@ -1 +1 @@".into(),
                    old_start: 1,
                    old_count: 1,
                    new_start: 1,
                    new_count: 1,
                    lines: vec![line(LineKind::Removed, Some(1), None)],
                },
                Hunk {
                    header: "@@ -5 +5 @@".into(),
                    old_start: 5,
                    old_count: 1,
                    new_start: 5,
                    new_count: 1,
                    lines: vec![line(LineKind::Added, None, Some(5))],
                },
            ],
            availability: PatchAvailability::Present,
        };
        let kinds: Vec<LineKind> = file.lines().map(|l| l.kind).collect();
        assert_eq!(kinds, vec![LineKind::Removed, LineKind::Added]);
    }
}
