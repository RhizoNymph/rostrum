//! One file's diff as the rows Kotlin renders, top to bottom.
//!
//! The anchoring rules are `rostrum-diff`'s, applied here and nowhere else:
//! each commentable line carries the [`DiffLine::anchor`] derived from that
//! line (added and context lines anchor their new line number on the right,
//! removed lines their old number on the left, and a line missing that
//! number carries none). Threads and drafts are placed after the line their
//! anchor names, matched on path, line and side together — the same number on
//! the other side is a different line.
//!
//! [`DiffLine::anchor`]: rostrum_diff::DiffLine::anchor

use rostrum_core::{RepoId, ReviewThread};
use rostrum_diff::{DiffFile, LineKind as CoreKind, hunk_word_changes};

use crate::{
    detail::thread_view,
    diff::{
        CommentAnchor, DiffLineView, DiffRow, LineKind,
        highlight::{file_highlights, plain_style},
        segments::segments,
    },
    review::book::{Draft, review_draft},
};

/// Every row of `file`: each hunk's header, its lines, and after each line
/// the threads and drafts anchored to it.
pub(crate) fn build_rows(
    file: &DiffFile,
    threads: &[ReviewThread],
    drafts: &[Draft],
    repo: &RepoId,
) -> Vec<DiffRow> {
    let highlights = file_highlights(file);
    let plain = plain_style();
    let mut rows = Vec::new();

    for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
        rows.push(DiffRow::Hunk {
            index: hunk_ix as u32,
            header: hunk.header.clone(),
        });
        let emphasis = hunk_word_changes(&hunk.lines);

        for (line_ix, line) in hunk.lines.iter().enumerate() {
            let anchor = line.anchor(&file.path);
            rows.push(DiffRow::Line {
                line: DiffLineView {
                    kind: match line.kind {
                        CoreKind::Context => LineKind::Context,
                        CoreKind::Added => LineKind::Added,
                        CoreKind::Removed => LineKind::Removed,
                    },
                    old_line: line.old_line,
                    new_line: line.new_line,
                    segments: segments(
                        &line.content,
                        &highlights[hunk_ix][line_ix],
                        &emphasis[line_ix],
                        plain,
                    ),
                    anchor: anchor.as_ref().map(|anchor| CommentAnchor {
                        path: anchor.path.clone(),
                        line: anchor.line,
                        side: anchor.side.into(),
                    }),
                    no_newline_at_eof: line.no_newline_at_eof,
                },
            });

            let Some(anchor) = anchor else {
                continue;
            };
            rows.extend(
                threads
                    .iter()
                    .filter(|thread| thread.is_anchored_at(&anchor.path, anchor.line, anchor.side))
                    .map(|thread| DiffRow::Thread {
                        thread: thread_view(thread, repo),
                    }),
            );
            rows.extend(
                drafts
                    .iter()
                    .filter(|draft| {
                        draft.comment.path == anchor.path
                            && draft.comment.side == anchor.side
                            && draft.comment.line == anchor.line
                    })
                    .map(|draft| DiffRow::Draft {
                        draft: review_draft(draft),
                    }),
            );
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use rostrum_core::{
        CommentId, DraftAnchor as CoreAnchor, Side as CoreSide, ThreadComment, ThreadId,
    };
    use rostrum_diff::{FileStatus, PatchAvailability, parse_patch};
    use rostrum_github::DraftComment;

    use super::*;
    use crate::types::Side;

    /// Context 10/10, removal of old 11, addition of new 11, context 12/12 —
    /// so line 11 exists on both sides and only the side tells them apart.
    fn file() -> DiffFile {
        DiffFile {
            path: "src/main.rs".into(),
            previous_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            hunks: parse_patch(
                "@@ -10,3 +10,3 @@ fn main() {\n ctx\n-let a = 1;\n+let a = 2;\n end\n",
            )
            .expect("patch"),
            availability: PatchAvailability::Present,
        }
    }

    fn repo() -> RepoId {
        "a/b".parse().expect("repo")
    }

    fn thread(id: &str, line: Option<u32>, side: CoreSide) -> ReviewThread {
        ReviewThread {
            id: ThreadId(id.into()),
            path: "src/main.rs".into(),
            line,
            original_line: line,
            side,
            is_resolved: false,
            is_outdated: line.is_none(),
            comments: vec![ThreadComment {
                id: CommentId(format!("{id}-c")),
                database_id: Some(1),
                author: None,
                body: "a **note**".into(),
                created_at: chrono::Utc::now(),
            }],
            opening_review: None,
        }
    }

    fn draft(id: u64, anchor: CoreAnchor) -> Draft {
        Draft {
            id,
            comment: DraftComment {
                path: anchor.path,
                line: anchor.line,
                side: anchor.side,
                start_line: anchor.start_line,
                start_side: anchor.start_side,
                body: "draft".into(),
            },
        }
    }

    /// A compact picture of the rows: `H`, `L<old>/<new>`, `T:<id>`, `D:<id>`.
    fn shape(rows: &[DiffRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                DiffRow::Hunk { .. } => "H".to_string(),
                DiffRow::Line { line } => format!(
                    "L{}/{}",
                    line.old_line.map_or("-".into(), |n| n.to_string()),
                    line.new_line.map_or("-".into(), |n| n.to_string())
                ),
                DiffRow::Thread { thread } => format!("T:{}", thread.id),
                DiffRow::Draft { draft } => format!("D:{}", draft.id),
            })
            .collect()
    }

    fn anchors(rows: &[DiffRow]) -> Vec<Option<(u32, Side)>> {
        rows.iter()
            .filter_map(|row| match row {
                DiffRow::Line { line } => Some(line.anchor.as_ref().map(|a| (a.line, a.side))),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_line_carries_the_anchor_github_expects() {
        let rows = build_rows(&file(), &[], &[], &repo());
        assert_eq!(
            shape(&rows),
            vec!["H", "L10/10", "L11/-", "L-/11", "L12/12"]
        );
        assert_eq!(
            anchors(&rows),
            vec![
                // Context anchors its new line on the right.
                Some((10, Side::Right)),
                // A removed line anchors its old line on the left.
                Some((11, Side::Left)),
                // An added line anchors its new line on the right.
                Some((11, Side::Right)),
                Some((12, Side::Right)),
            ]
        );
        let DiffRow::Line { line } = &rows[1] else {
            panic!("line");
        };
        assert_eq!(
            line.anchor.as_ref().map(|a| a.path.as_str()),
            Some("src/main.rs")
        );
    }

    #[test]
    fn a_line_missing_its_number_is_not_commentable() {
        // A hunk starting at 0 on the new side (a malformed header) leaves the
        // added line without a new number.
        let mut broken = file();
        broken.hunks = parse_patch("@@ -1,1 +0,1 @@\n+orphan\n").expect("patch");
        let rows = build_rows(&broken, &[], &[], &repo());
        assert_eq!(anchors(&rows), vec![None]);
    }

    #[test]
    fn threads_and_drafts_follow_their_line_and_side() {
        let threads = vec![
            thread("right11", Some(11), CoreSide::Right),
            thread("left11", Some(11), CoreSide::Left),
            thread("outdated", None, CoreSide::Right),
            ReviewThread {
                path: "src/other.rs".into(),
                ..thread("elsewhere", Some(10), CoreSide::Right)
            },
        ];
        let drafts = vec![
            draft(1, CoreAnchor::single("src/main.rs", 10, CoreSide::Right)),
            // A range attaches after its last line.
            draft(
                2,
                CoreAnchor::single("src/main.rs", 10, CoreSide::Right)
                    .extended_to(12, CoreSide::Right),
            ),
            draft(3, CoreAnchor::single("src/main.rs", 11, CoreSide::Left)),
        ];
        let rows = build_rows(&file(), &threads, &drafts, &repo());
        assert_eq!(
            shape(&rows),
            vec![
                "H",
                "L10/10",
                "D:1",
                "L11/-",
                "T:left11",
                "D:3",
                "L-/11",
                "T:right11",
                "L12/12",
                "D:2",
            ]
        );
        let DiffRow::Thread { thread } = &rows[4] else {
            panic!("thread row");
        };
        assert!(
            thread.comments[0].body[0]
                .spans
                .iter()
                .any(|span| span.bold)
        );
    }

    #[test]
    fn changed_words_are_emphasised_in_the_pair() {
        let rows = build_rows(&file(), &[], &[], &repo());
        let emphasized = |ix: usize| -> Vec<String> {
            let DiffRow::Line { line } = &rows[ix] else {
                panic!("line");
            };
            line.segments
                .iter()
                .filter(|segment| segment.emphasized)
                .map(|segment| segment.text.clone())
                .collect()
        };
        assert_eq!(emphasized(2), vec!["1"]);
        assert_eq!(emphasized(3), vec!["2"]);
        assert!(emphasized(1).is_empty());
    }

    #[test]
    fn segments_rebuild_each_line() {
        let diff = file();
        let rows = build_rows(&diff, &[], &[], &repo());
        let texts: Vec<String> = rows
            .iter()
            .filter_map(|row| match row {
                DiffRow::Line { line } => {
                    Some(line.segments.iter().map(|s| s.text.as_str()).collect())
                }
                _ => None,
            })
            .collect();
        let expected: Vec<String> = diff.hunks[0]
            .lines
            .iter()
            .map(|l| l.content.clone())
            .collect();
        assert_eq!(texts, expected);
    }
}
