//! The Files tab: the diff, inline threads, and the inline comment composer.
//!
//! Like the feed, every file, hunk, line, thread, and composer is flattened
//! into one row stream rendered by a single virtualized `list`. A large pull
//! request is tens of thousands of rows, and GPUI shapes text for every element
//! actually constructed, so building only the visible rows is not optional.

use std::collections::HashSet;

use gpui::{
    AnyElement, Context, Hsla, SharedString, StyledText, TextRun, Window, div, list, prelude::*,
    px, rems,
};
use rostrum_core::ReviewThread;
use rostrum_diff::{DiffFile, HighlightSpan, LineKind, PatchAvailability};
use rostrum_github::DraftComment;
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, ButtonStyle, Chip, DiffStat, h_flex, v_flex},
};

use crate::detail::{DraftAnchor, Loadable, PrDetail};

/// One row of the flattened diff stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRow {
    FileHeader {
        file: usize,
    },
    /// The diff could not be shown (binary, too large, or unparseable).
    Unavailable {
        file: usize,
    },
    HunkHeader {
        file: usize,
        hunk: usize,
    },
    Line {
        file: usize,
        hunk: usize,
        line: usize,
    },
    /// An existing review thread, indexed into `Conversation::threads`.
    Thread {
        file: usize,
        thread: usize,
    },
    /// A locally drafted comment, indexed into `PrDetail::pending`.
    Draft {
        file: usize,
        draft: usize,
    },
    /// The open inline composer.
    Composer {
        file: usize,
    },
    Spacer,
}

/// Rebuild the row stream from a `PrDetail`'s current state.
pub fn build_rows(detail: &PrDetail) -> Vec<DiffRow> {
    let Some(files) = detail.files.loaded() else {
        return Vec::new();
    };
    let threads: &[ReviewThread] = detail
        .conversation
        .loaded()
        .map(|c| c.threads.as_slice())
        .unwrap_or(&[]);

    flatten(
        files,
        threads,
        &detail.pending,
        detail.inline.as_ref().map(|(anchor, _)| anchor),
        &detail.collapsed,
    )
}

/// The pure core of [`build_rows`], split out so the interleaving of threads,
/// drafts, and the composer can be tested without a window.
pub fn flatten(
    files: &[DiffFile],
    threads: &[ReviewThread],
    pending: &[DraftComment],
    inline: Option<&DraftAnchor>,
    collapsed: &HashSet<usize>,
) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    tracing::trace!(
        files = files.len(),
        threads = threads.len(),
        "building diff rows"
    );

    for (file_ix, file) in files.iter().enumerate() {
        rows.push(DiffRow::FileHeader { file: file_ix });

        if collapsed.contains(&file_ix) {
            rows.push(DiffRow::Spacer);
            continue;
        }

        if file.availability != PatchAvailability::Present || file.hunks.is_empty() {
            rows.push(DiffRow::Unavailable { file: file_ix });
            rows.push(DiffRow::Spacer);
            continue;
        }

        for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
            rows.push(DiffRow::HunkHeader {
                file: file_ix,
                hunk: hunk_ix,
            });

            for (line_ix, line) in hunk.lines.iter().enumerate() {
                rows.push(DiffRow::Line {
                    file: file_ix,
                    hunk: hunk_ix,
                    line: line_ix,
                });

                let Some(anchor) = line.anchor(&file.path) else {
                    continue;
                };

                // Existing threads anchored to this line.
                for (thread_ix, thread) in threads.iter().enumerate() {
                    if thread.path == anchor.path
                        && thread.side == anchor.side
                        && thread.line == Some(anchor.line)
                    {
                        rows.push(DiffRow::Thread {
                            file: file_ix,
                            thread: thread_ix,
                        });
                    }
                }

                // Comments drafted locally but not yet submitted.
                for (draft_ix, draft) in pending.iter().enumerate() {
                    if draft.path == anchor.path
                        && draft.side == anchor.side
                        && draft.line == anchor.line
                    {
                        rows.push(DiffRow::Draft {
                            file: file_ix,
                            draft: draft_ix,
                        });
                    }
                }

                if let Some(open) = inline
                    && open.path == anchor.path
                    && open.side == anchor.side
                    && open.line == anchor.line
                {
                    rows.push(DiffRow::Composer { file: file_ix });
                }
            }
        }

        rows.push(DiffRow::Spacer);
    }

    rows
}

pub fn render(detail: &PrDetail, cx: &Context<PrDetail>) -> AnyElement {
    let theme = cx.theme().clone();

    match &detail.files {
        Loadable::Idle | Loadable::Loading => {
            return centered("Loading diff…", theme.text_subtle).into_any_element();
        }
        Loadable::Failed(message) => {
            return centered(message.clone(), theme.danger).into_any_element();
        }
        Loadable::Loaded(files) if files.is_empty() => {
            return centered("No files changed", theme.text_subtle).into_any_element();
        }
        Loadable::Loaded(_) => {}
    }

    let entity = cx.entity();
    div()
        .size_full()
        .child(
            list(detail.diff_list.clone(), move |ix, window, cx| {
                entity.update(cx, |detail, cx| render_row(detail, ix, window, cx))
            })
            .size_full(),
        )
        .into_any_element()
}

fn render_row(
    detail: &PrDetail,
    ix: usize,
    window: &mut Window,
    cx: &mut Context<PrDetail>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let Some(row) = detail.diff_rows.get(ix).cloned() else {
        return div().into_any_element();
    };
    let Some(files) = detail.files.loaded() else {
        return div().into_any_element();
    };

    match row {
        DiffRow::Spacer => div().h(px(12.)).into_any_element(),

        DiffRow::FileHeader { file } => {
            let Some(diff) = files.get(file) else {
                return div().into_any_element();
            };
            let collapsed = detail.collapsed.contains(&file);
            let path = diff.path.clone();
            let renamed = diff.previous_path.clone();

            h_flex()
                .id(("file-header", file))
                .gap_2()
                .px_3()
                .py_2()
                .bg(theme.surface_raised)
                .border_1()
                .border_color(theme.border)
                .rounded_tl(px(6.))
                .rounded_tr(px(6.))
                .cursor_pointer()
                .child(
                    div()
                        .text_color(theme.text_subtle)
                        .text_size(rems(0.7))
                        .child(if collapsed { "▸" } else { "▾" }),
                )
                .child(
                    div()
                        .flex_1()
                        .font_family(theme.mono_font.clone())
                        .text_size(rems(0.76))
                        .text_color(theme.text)
                        .child(match renamed {
                            Some(previous) => format!("{previous} → {path}"),
                            None => path,
                        }),
                )
                .child(Chip::new(format!("{:?}", diff.status).to_lowercase()))
                .child(DiffStat::new(diff.additions, diff.deletions))
                .on_click(PrDetail::on_click(cx, move |this, cx| {
                    this.toggle_file(file, cx)
                }))
                .into_any_element()
        }

        DiffRow::Unavailable { file } => {
            let reason =
                files
                    .get(file)
                    .map_or("Diff unavailable", |diff| match diff.availability {
                        PatchAvailability::Omitted => "Diff not provided (binary or too large)",
                        PatchAvailability::Truncated => "Diff could not be parsed",
                        PatchAvailability::Present => "No changes to display",
                    });
            side_bordered(&theme)
                .px_3()
                .py_2()
                .text_size(rems(0.76))
                .text_color(theme.text_subtle)
                .child(reason)
                .into_any_element()
        }

        DiffRow::HunkHeader { file, hunk } => {
            let header = files
                .get(file)
                .and_then(|diff| diff.hunks.get(hunk))
                .map(|hunk| hunk.header.clone())
                .unwrap_or_default();

            side_bordered(&theme)
                .px_3()
                .py_1()
                .bg(theme.surface_raised)
                .font_family(theme.mono_font.clone())
                .text_size(rems(0.72))
                .text_color(theme.text_subtle)
                .child(header)
                .into_any_element()
        }

        DiffRow::Line { file, hunk, line } => {
            let Some(diff) = files.get(file) else {
                return div().into_any_element();
            };
            let Some(source) = diff.hunks.get(hunk).and_then(|h| h.lines.get(line)) else {
                return div().into_any_element();
            };

            let (background, gutter) = match source.kind {
                LineKind::Added => (
                    Some(Hsla {
                        a: 0.12,
                        ..theme.added
                    }),
                    theme.added,
                ),
                LineKind::Removed => (
                    Some(Hsla {
                        a: 0.12,
                        ..theme.removed
                    }),
                    theme.removed,
                ),
                LineKind::Context => (None, theme.text_subtle),
            };
            let marker = match source.kind {
                LineKind::Added => "+",
                LineKind::Removed => "-",
                LineKind::Context => " ",
            };

            let anchor = source.anchor(&diff.path).map(|anchor| DraftAnchor {
                path: anchor.path,
                line: anchor.line,
                side: anchor.side,
            });

            let spans = detail
                .highlighter
                .highlight_lines_for_path(&diff.path, &[source.content.as_str()])
                .into_iter()
                .next()
                .unwrap_or_default();
            let content = styled_code(&source.content, &spans, &theme, window);

            h_flex()
                .id(("line", ix))
                .items_start()
                .font_family(theme.mono_font.clone())
                .text_size(rems(0.74))
                .when_some(background, |el, color| el.bg(color))
                .border_l_1()
                .border_r_1()
                .border_color(theme.border)
                .child(gutter_cell(source.old_line, gutter, &theme))
                .child(gutter_cell(source.new_line, gutter, &theme))
                .child(
                    div()
                        .w(px(14.))
                        .flex_none()
                        .text_color(gutter)
                        .child(marker),
                )
                .child(div().flex_1().min_w_0().child(content))
                .when_some(anchor, |el, anchor| {
                    // Only commentable lines get the affordance; a line with no
                    // usable line number cannot be anchored to.
                    el.child(
                        div()
                            .flex_none()
                            .px_1()
                            .text_color(theme.text_subtle)
                            .text_size(rems(0.7))
                            .child("+")
                            .id(("comment-on", ix))
                            .cursor_pointer()
                            .on_click(PrDetail::on_click(cx, move |this, cx| {
                                this.open_inline_composer(anchor.clone(), cx)
                            })),
                    )
                })
                .into_any_element()
        }

        DiffRow::Thread { thread, .. } => {
            let Some(review) = detail
                .conversation
                .loaded()
                .and_then(|c| c.threads.get(thread))
            else {
                return div().into_any_element();
            };

            side_bordered(&theme)
                .p_2()
                .bg(theme.background)
                .child(
                    v_flex()
                        .gap_1()
                        .ml_4()
                        .p_2()
                        .rounded_tl(px(6.))
                        .rounded_tr(px(6.))
                        .rounded_bl(px(6.))
                        .rounded_br(px(6.))
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            h_flex()
                                .gap_2()
                                .text_size(rems(0.7))
                                .text_color(theme.text_subtle)
                                .child(format!("{} comment(s)", review.comments.len()))
                                .when(review.is_resolved, |el| {
                                    el.child(Chip::new("resolved").color(theme.success))
                                }),
                        )
                        .children(review.comments.iter().map(|comment| {
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(rems(0.7))
                                        .text_color(theme.text_muted)
                                        .child(
                                            comment
                                                .author
                                                .as_ref()
                                                .map(|a| a.login.clone())
                                                .unwrap_or_else(|| "unknown".into()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(rems(0.76))
                                        .text_color(theme.text)
                                        .child(comment.body.clone()),
                                )
                        })),
                )
                .into_any_element()
        }

        DiffRow::Draft { draft, .. } => {
            let Some(pending) = detail.pending.get(draft) else {
                return div().into_any_element();
            };
            let body = pending.body.clone();

            side_bordered(&theme)
                .p_2()
                .child(
                    v_flex()
                        .ml_4()
                        .gap_1()
                        .p_2()
                        .rounded_tl(px(6.))
                        .rounded_tr(px(6.))
                        .rounded_bl(px(6.))
                        .rounded_br(px(6.))
                        .border_1()
                        .border_color(theme.warning)
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Chip::new("pending").color(theme.warning))
                                .child(div().flex_1())
                                .child(Button::new(("discard-draft", draft), "Discard").on_click(
                                    PrDetail::on_click(cx, move |this, cx| {
                                        this.discard_draft(draft, cx)
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .text_size(rems(0.76))
                                .text_color(theme.text)
                                .child(body),
                        ),
                )
                .into_any_element()
        }

        DiffRow::Composer { .. } => {
            let Some((_, input)) = detail.inline.as_ref() else {
                return div().into_any_element();
            };

            side_bordered(&theme)
                .p_2()
                .child(
                    v_flex().ml_4().gap_2().child(input.clone()).child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("add-draft", "Add to review")
                                    .style(ButtonStyle::Primary)
                                    .on_click(PrDetail::on_click(cx, |this, cx| {
                                        this.commit_inline_draft(cx)
                                    })),
                            )
                            .child(Button::new("cancel-draft", "Cancel").on_click(
                                PrDetail::on_click(cx, |this, cx| this.discard_inline_draft(cx)),
                            )),
                    ),
                )
                .into_any_element()
        }
    }
}

fn side_bordered(theme: &Theme) -> gpui::Div {
    div()
        .border_l_1()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface)
}

fn gutter_cell(number: Option<u32>, color: Hsla, theme: &Theme) -> impl IntoElement {
    div()
        .w(px(44.))
        .flex_none()
        .px_1()
        .text_color(if number.is_some() {
            color
        } else {
            theme.border
        })
        .child(number.map(|n| n.to_string()).unwrap_or_default())
}

/// Build a shaped run per highlight span.
///
/// `HighlightSpan::len` is in UTF-8 bytes and the spans tile the line exactly,
/// which is what gpui requires of `TextRun`s — a gap or overlap would panic.
fn styled_code(
    content: &str,
    spans: &[HighlightSpan],
    theme: &Theme,
    window: &Window,
) -> AnyElement {
    if content.is_empty() {
        return div().into_any_element();
    }

    let font = gpui::font(theme.mono_font.clone());
    let base = window.text_style();

    let mut runs: Vec<TextRun> = spans
        .iter()
        .filter(|span| span.len > 0)
        .map(|span| TextRun {
            len: span.len,
            font: font.clone(),
            color: Hsla::from(gpui::rgb(
                ((span.style.r as u32) << 16) | ((span.style.g as u32) << 8) | span.style.b as u32,
            )),
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect();

    // Defensive: if highlighting produced nothing usable, fall back to one
    // unstyled run rather than risking a coverage mismatch.
    let covered: usize = runs.iter().map(|run| run.len).sum();
    if runs.is_empty() || covered != content.len() {
        runs = vec![TextRun {
            len: content.len(),
            font,
            color: base.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    StyledText::new(SharedString::from(content.to_string()))
        .with_runs(runs)
        .into_any_element()
}

fn centered(message: impl Into<String>, color: Hsla) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(rems(0.82))
        .text_color(color)
        .child(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rostrum_core::{Side, ThreadId};
    use rostrum_diff::{DiffLine, FileStatus, Hunk};

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            old_line: old,
            new_line: new,
            content: "x".into(),
            no_newline_at_eof: false,
        }
    }

    /// One file: context line 10/10, a removal of old 11, an addition of new 11.
    fn file() -> DiffFile {
        DiffFile {
            path: "src/main.rs".into(),
            previous_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            availability: PatchAvailability::Present,
            hunks: vec![Hunk {
                header: "@@ -10,2 +10,2 @@".into(),
                old_start: 10,
                old_count: 2,
                new_start: 10,
                new_count: 2,
                lines: vec![
                    line(LineKind::Context, Some(10), Some(10)),
                    line(LineKind::Removed, Some(11), None),
                    line(LineKind::Added, None, Some(11)),
                ],
            }],
        }
    }

    fn thread(path: &str, line: Option<u32>, side: Side) -> ReviewThread {
        ReviewThread {
            id: ThreadId("t".into()),
            path: path.into(),
            line,
            original_line: line,
            side,
            is_resolved: false,
            is_outdated: false,
            comments: Vec::new(),
        }
    }

    fn rows(
        threads: &[ReviewThread],
        pending: &[DraftComment],
        inline: Option<&DraftAnchor>,
    ) -> Vec<DiffRow> {
        flatten(&[file()], threads, pending, inline, &HashSet::new())
    }

    #[test]
    fn emits_header_hunk_lines_and_spacer() {
        assert_eq!(
            rows(&[], &[], None),
            vec![
                DiffRow::FileHeader { file: 0 },
                DiffRow::HunkHeader { file: 0, hunk: 0 },
                DiffRow::Line {
                    file: 0,
                    hunk: 0,
                    line: 0
                },
                DiffRow::Line {
                    file: 0,
                    hunk: 0,
                    line: 1
                },
                DiffRow::Line {
                    file: 0,
                    hunk: 0,
                    line: 2
                },
                DiffRow::Spacer,
            ]
        );
    }

    #[test]
    fn collapsed_file_contributes_header_and_spacer_only() {
        let collapsed = HashSet::from([0usize]);
        assert_eq!(
            flatten(&[file()], &[], &[], None, &collapsed),
            vec![DiffRow::FileHeader { file: 0 }, DiffRow::Spacer]
        );
    }

    #[test]
    fn missing_patch_yields_an_unavailable_row() {
        let mut file = file();
        file.availability = PatchAvailability::Omitted;
        file.hunks.clear();
        assert_eq!(
            flatten(&[file], &[], &[], None, &HashSet::new()),
            vec![
                DiffRow::FileHeader { file: 0 },
                DiffRow::Unavailable { file: 0 },
                DiffRow::Spacer,
            ]
        );
    }

    /// A thread on the new file attaches under the line carrying that new
    /// number on the right-hand side.
    #[test]
    fn thread_attaches_after_its_right_side_line() {
        let rows = rows(&[thread("src/main.rs", Some(11), Side::Right)], &[], None);
        let position = rows
            .iter()
            .position(|row| matches!(row, DiffRow::Thread { .. }))
            .expect("thread row present");
        // Line index 2 is the addition whose new_line is 11.
        assert_eq!(
            rows[position - 1],
            DiffRow::Line {
                file: 0,
                hunk: 0,
                line: 2
            }
        );
    }

    /// The same line number on the other side is a different anchor entirely.
    #[test]
    fn thread_side_disambiguates_identical_line_numbers() {
        let rows = rows(&[thread("src/main.rs", Some(11), Side::Left)], &[], None);
        let position = rows
            .iter()
            .position(|row| matches!(row, DiffRow::Thread { .. }))
            .expect("thread row present");
        // Line index 1 is the removal whose old_line is 11.
        assert_eq!(
            rows[position - 1],
            DiffRow::Line {
                file: 0,
                hunk: 0,
                line: 1
            }
        );
    }

    #[test]
    fn threads_for_other_files_and_outdated_threads_are_not_attached() {
        assert!(
            !rows(&[thread("other.rs", Some(11), Side::Right)], &[], None)
                .iter()
                .any(|row| matches!(row, DiffRow::Thread { .. }))
        );
        assert!(
            !rows(&[thread("src/main.rs", None, Side::Right)], &[], None)
                .iter()
                .any(|row| matches!(row, DiffRow::Thread { .. }))
        );
    }

    #[test]
    fn pending_draft_attaches_to_its_anchor() {
        let draft = DraftComment::single("src/main.rs", 10, Side::Right, "looks wrong");
        let rows = rows(&[], std::slice::from_ref(&draft), None);
        let position = rows
            .iter()
            .position(|row| matches!(row, DiffRow::Draft { .. }))
            .expect("draft row present");
        assert_eq!(
            rows[position - 1],
            DiffRow::Line {
                file: 0,
                hunk: 0,
                line: 0
            }
        );
    }

    #[test]
    fn open_composer_attaches_to_its_anchor() {
        let anchor = DraftAnchor {
            path: "src/main.rs".into(),
            line: 11,
            side: Side::Left,
        };
        let rows = rows(&[], &[], Some(&anchor));
        let position = rows
            .iter()
            .position(|row| matches!(row, DiffRow::Composer { .. }))
            .expect("composer row present");
        assert_eq!(
            rows[position - 1],
            DiffRow::Line {
                file: 0,
                hunk: 0,
                line: 1
            }
        );
    }

    #[test]
    fn no_files_yields_no_rows() {
        assert!(flatten(&[], &[], &[], None, &HashSet::new()).is_empty());
    }
}
