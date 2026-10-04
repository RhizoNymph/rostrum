//! Drawing the CI view: the toolbar, the grid's lines, and the log pane.
//!
//! The grid is one virtualized `list` over [`GridLine`]s, sized to the widest
//! section so that it scrolls horizontally as a whole when columns overflow:
//! every row header and column header moves with its cells.

use chrono::Utc;
use gpui::{AnyElement, Context, Hsla, Window, div, list, prelude::*, px, rems};
use rostrum_core::{
    StackPlace,
    ci::{CellRef, CheckEntry, CheckStatus, GridLine, GridRow, GridSection, RollupState, Timing},
};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, ButtonStyle, Checkbox, Dot, TextTooltip, h_flex, v_flex},
};

use super::{CELL_WIDTH, CI_CONTEXT, CiEvent, CiView, ROW_HEADER_WIDTH};

/// A status's tile colour.
fn status_color(status: CheckStatus, theme: &Theme) -> Hsla {
    match status {
        CheckStatus::Success => theme.success,
        CheckStatus::Failure | CheckStatus::TimedOut => theme.danger,
        CheckStatus::ActionRequired => theme.warning,
        CheckStatus::InProgress | CheckStatus::Queued => theme.accent,
        CheckStatus::Cancelled | CheckStatus::Skipped | CheckStatus::Neutral => theme.text_subtle,
    }
}

fn rollup_color(state: RollupState, theme: &Theme) -> Hsla {
    match state {
        RollupState::Failing => theme.danger,
        RollupState::Running => theme.accent,
        RollupState::Passing => theme.success,
        RollupState::Settled | RollupState::Empty => theme.text_subtle,
    }
}

/// A one-glyph mark for a status, so a tile reads without colour too.
fn glyph(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Success => "✓",
        CheckStatus::Failure => "✗",
        CheckStatus::TimedOut => "⏱",
        CheckStatus::ActionRequired => "!",
        CheckStatus::InProgress => "●",
        CheckStatus::Queued => "○",
        CheckStatus::Cancelled => "⊘",
        CheckStatus::Skipped => "↷",
        CheckStatus::Neutral => "–",
    }
}

impl CiView {
    fn render_line(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let grid = self.grid.clone();
        let Some(line) = grid.lines().get(ix) else {
            return div().into_any_element();
        };
        match line {
            GridLine::Header { section } => self.render_header(&grid.sections()[*section], &theme),
            GridLine::Stack { members, .. } => h_flex()
                .h(px(22.))
                .pl_3()
                .text_size(rems(0.72))
                .text_color(theme.text_subtle)
                .child(format!("Stack · {members} PRs"))
                .into_any_element(),
            GridLine::Row { section, row } => {
                let section = &grid.sections()[*section];
                self.render_row(section, &section.rows[*row], &theme, cx)
            }
            GridLine::Notice { section } => {
                let section = &grid.sections()[*section];
                let text = match &section.load {
                    rostrum_core::LoadState::Failed { message, .. } => message.clone(),
                    rostrum_core::LoadState::Idle | rostrum_core::LoadState::Loading => {
                        "Loading checks…".into()
                    }
                    rostrum_core::LoadState::Loaded { .. } => "No open pull requests".into(),
                };
                div()
                    .px_3()
                    .py_2()
                    .text_size(rems(0.76))
                    .text_color(theme.text_subtle)
                    .child(text)
                    .into_any_element()
            }
            GridLine::Spacer => div().h(px(12.)).into_any_element(),
        }
    }

    fn render_header(&self, section: &GridSection, theme: &Theme) -> AnyElement {
        let failed = section.load.error_message().map(str::to_string);
        h_flex()
            .h(px(52.))
            .items_end()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface_raised)
            .child(
                v_flex()
                    .w(px(ROW_HEADER_WIDTH))
                    .flex_none()
                    .px_3()
                    .pb_1()
                    .child(
                        div()
                            .text_size(rems(0.85))
                            .text_color(theme.text)
                            .child(section.repo.to_string()),
                    )
                    .child(
                        div()
                            .text_size(rems(0.68))
                            .text_color(if failed.is_some() {
                                theme.danger
                            } else {
                                theme.text_subtle
                            })
                            .child(match (&failed, section.hidden) {
                                (Some(message), _) => message.clone(),
                                (None, 0) => format!("{} checks", section.columns.len()),
                                (None, hidden) => {
                                    format!(
                                        "{} checks · {hidden} rows hidden",
                                        section.columns.len()
                                    )
                                }
                            }),
                    ),
            )
            .children(section.columns.iter().map(|column| {
                // The job name on the last line, the workflow above it: the
                // job is what differs column to column.
                v_flex()
                    .w(px(CELL_WIDTH))
                    .flex_none()
                    .px_1()
                    .pb_1()
                    .overflow_hidden()
                    .when_some(column.workflow.clone(), |el, workflow| {
                        el.child(
                            div()
                                .truncate()
                                .text_size(rems(0.62))
                                .text_color(theme.text_subtle)
                                .child(workflow),
                        )
                    })
                    .child(
                        div()
                            .truncate()
                            .text_size(rems(0.7))
                            .text_color(theme.text_muted)
                            .child(column.name.clone()),
                    )
            }))
            .into_any_element()
    }

    fn render_row(
        &self,
        section: &GridSection,
        row: &GridRow,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Utc::now();
        let selected = self.selected.as_ref();
        let stack_mark = row.stack.map(|place| match place {
            StackPlace::Bottom => "┗",
            StackPlace::Middle => "┃",
            StackPlace::Top => "┏",
            StackPlace::Only => "•",
        });
        h_flex()
            .h(px(40.))
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .w(px(ROW_HEADER_WIDTH))
                    .flex_none()
                    .px_3()
                    .gap_2()
                    .child(Dot::new(rollup_color(row.rollup.state(), theme)))
                    .when_some(stack_mark, |el, mark| {
                        el.child(div().text_color(theme.text_subtle).child(mark))
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .text_size(rems(0.76))
                                    .child(
                                        div()
                                            .text_color(theme.text_subtle)
                                            .child(row.number.to_string()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_color(theme.text)
                                            .child(row.title.clone()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .text_size(rems(0.66))
                                    .text_color(theme.text_subtle)
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .child(row.head_sha.clone()),
                                    )
                                    .child(if row.fetched {
                                        row.rollup.describe()
                                    } else {
                                        "checks not loaded".into()
                                    })
                                    .when(row.truncated, |el| el.child("· more not shown")),
                            ),
                    ),
            )
            .children(
                row.cells
                    .iter()
                    .zip(&section.columns)
                    .map(|(cell, column)| {
                        let at = CellRef {
                            repo: section.repo.clone(),
                            number: row.number,
                            column: column.clone(),
                        };
                        let is_selected = selected == Some(&at);
                        self.render_cell(cell.as_ref(), at, is_selected, now, theme, cx)
                    }),
            )
            .into_any_element()
    }

    fn render_cell(
        &self,
        entry: Option<&CheckEntry>,
        at: CellRef,
        selected: bool,
        now: chrono::DateTime<Utc>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id =
            gpui::ElementId::Name(format!("ci-{}-{}-{}", at.repo, at.number.0, at.column).into());
        let base = div()
            .id(id)
            .w(px(CELL_WIDTH))
            .h_full()
            .flex_none()
            .p_0p5()
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, _window, cx| {
                    this.select(at.clone(), cx);
                    if event.click_count() >= 2 {
                        this.open_log(cx);
                    }
                }),
            );

        let tile = div()
            .size_full()
            .px_1p5()
            .rounded(px(4.))
            .border_1()
            .border_color(if selected { theme.accent } else { theme.border });

        let Some(entry) = entry else {
            return base
                .child(
                    tile.flex()
                        .items_center()
                        .text_size(rems(0.66))
                        .text_color(theme.text_subtle)
                        .child("not run"),
                )
                .into_any_element();
        };

        let timing = Timing::of(entry, now);
        let color = status_color(entry.status, theme);
        let tooltip = [
            Some(format!("{} — {}", entry.key, entry.status.label())),
            timing.duration_label(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");

        base.tooltip(move |_window, cx| {
            cx.new(|_| TextTooltip {
                text: tooltip.clone().into(),
            })
            .into()
        })
        .child(
            tile.bg(Hsla { a: 0.14, ..color })
                .when(selected, |el| el.border_2())
                .child(
                    h_flex()
                        .gap_1()
                        .text_size(rems(0.7))
                        .text_color(color)
                        .child(glyph(entry.status))
                        .child(div().truncate().child(entry.status.label())),
                )
                .when_some(timing.label(), |el, label| {
                    el.child(
                        div()
                            .truncate()
                            .text_size(rems(0.64))
                            .text_color(theme.text_muted)
                            .child(label),
                    )
                }),
        )
        .into_any_element()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let attention = self.filter.needs_attention;
        let running = self.store.read(cx).ci.any_running();
        h_flex()
            .flex_none()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("ci-back", "← Feed")
                    .tooltip("Back to the feed (escape or shift-c)")
                    .on_click(cx.listener(|_, _, _window, cx| cx.emit(CiEvent::Leave))),
            )
            .child(div().text_size(rems(0.9)).child("CI"))
            .child(
                Checkbox::new("ci-attention", "failing or running only (f)", attention)
                    .on_toggle(cx.listener(|this, _, _window, cx| this.toggle_attention(cx))),
            )
            .child(div().flex_1())
            .when(running, |el| {
                el.child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.text_subtle)
                        .child("checks running — refreshing every 15s"),
                )
            })
            .child(
                div()
                    .text_size(rems(0.7))
                    .text_color(theme.text_subtle)
                    .child("hjkl move · enter log · r re-run"),
            )
    }

    /// The confirmation strip and the last re-run's outcome.
    fn render_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        if let Some(pending) = &self.confirm {
            let entry = self.grid.entry(&pending.cell)?.clone();
            let prompt = pending
                .targets
                .first()
                .map(|target| target.confirm_prompt(&entry))
                .unwrap_or_default();
            return Some(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(theme.surface_raised)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().flex_1().text_size(rems(0.8)).child(prompt))
                    .children(pending.targets.iter().enumerate().map(|(ix, target)| {
                        let target = *target;
                        Button::new(("ci-rerun", ix), target.label())
                            .style(if ix == 0 {
                                ButtonStyle::Primary
                            } else {
                                ButtonStyle::Subtle
                            })
                            .tooltip(target.confirm_prompt(&entry))
                            .on_click(
                                cx.listener(move |this, _, _window, cx| this.rerun(target, cx)),
                            )
                    }))
                    .child(
                        Button::new("ci-rerun-cancel", "Cancel").on_click(cx.listener(
                            |this, _, _window, cx| {
                                this.confirm = None;
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }
        let (ok, message) = self.notice.clone()?;
        Some(
            div()
                .flex_none()
                .px_3()
                .py_1()
                .text_size(rems(0.76))
                .text_color(if ok { theme.text_muted } else { theme.danger })
                .child(message)
                .into_any_element(),
        )
    }
}

impl Render for CiView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let width = ROW_HEADER_WIDTH + CELL_WIDTH * self.grid.max_columns() as f32 + 24.;
        let banner = self.render_banner(cx);
        let log = self.log.as_ref().map(|(view, _)| view.clone());

        v_flex()
            .size_full()
            .key_context(CI_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_retry))
            .on_action(cx.listener(Self::on_toggle_attention))
            .on_action(cx.listener(Self::on_dismiss))
            .on_action(cx.listener(Self::on_leave))
            .child(self.render_toolbar(cx))
            .children(banner)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("ci-grid-scroll")
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .overflow_x_scroll()
                            .child(
                                div().w(px(width)).h_full().child(
                                    list(
                                        self.list.clone(),
                                        cx.processor(|this, ix: usize, _window, cx| {
                                            this.render_line(ix, cx)
                                        }),
                                    )
                                    .size_full(),
                                ),
                            ),
                    )
                    .when_some(log, |el, log| {
                        el.child(div().w(px(1.)).h_full().bg(theme.border))
                            .child(div().w(px(640.)).h_full().flex_none().child(log))
                    }),
            )
    }
}
