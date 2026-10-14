//! Drawing the matrix: the one header row, and each line of the list — stack
//! headings and pull request rows with their cells.
//!
//! The row-header column is fixed; the check columns to its right are drawn
//! shifted left by the view's horizontal offset and clipped, and only the
//! columns in view are drawn ([`super::hscroll::HScroll::visible`]). The
//! header row is drawn the same way above the list, so a column header and
//! its cells always share an x position.

use std::ops::Range;

use chrono::Utc;
use gpui::{AnyElement, Context, Hsla, div, prelude::*, px, rems};
use rostrum_core::{
    StackPlace,
    ci::{CellRef, CheckEntry, CheckStatus, GridLine, GridRow, RollupState, Timing},
};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Dot, TextTooltip, h_flex, v_flex},
};

use super::{CELL_WIDTH, CiView, ROW_HEADER_WIDTH};

/// Height of the header's workflow band and of its job-name band.
const GROUP_BAND: f32 = 20.;
const NAME_BAND: f32 = 30.;

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

/// The clipped strip right of the row-header column, holding `content`
/// shifted so that column `first` sits at its x position.
fn columns_strip(first: usize, offset: f32, content: impl IntoElement) -> impl IntoElement {
    div().flex_1().min_w_0().h_full().overflow_hidden().child(
        h_flex()
            .h_full()
            .relative()
            .left(px(CELL_WIDTH * first as f32 - offset))
            .child(content),
    )
}

impl CiView {
    /// The columns in view, with the offset they are drawn at.
    fn in_view(&self) -> (Range<usize>, f32) {
        let range =
            self.hscroll
                .visible(self.grid.columns().len(), CELL_WIDTH, self.columns_viewport);
        (range, self.hscroll.offset())
    }

    /// The one header row: a corner summarising the grid, workflow names
    /// over their jobs, then each job's name.
    pub(super) fn render_header(&self, theme: &Theme) -> AnyElement {
        let grid = &self.grid;
        let (range, offset) = self.in_view();
        let groups: Vec<_> = grid
            .column_groups()
            .iter()
            .filter(|g| g.columns.start < range.end && g.columns.end > range.start)
            .collect();
        let first_group = groups.first().map_or(0, |g| g.columns.start);
        let hidden = grid.hidden();
        let summary = match hidden {
            0 => format!(
                "{} pull requests · {} checks",
                grid.rows().len(),
                grid.columns().len()
            ),
            hidden => format!(
                "{} pull requests · {} checks · {hidden} hidden",
                grid.rows().len(),
                grid.columns().len()
            ),
        };

        let group_band = columns_strip(
            first_group,
            offset,
            h_flex().children(groups.iter().map(|group| {
                let width = CELL_WIDTH * group.columns.len() as f32;
                div()
                    .w(px(width))
                    .flex_none()
                    .h(px(GROUP_BAND))
                    .px_1()
                    .border_l_1()
                    .border_color(theme.border)
                    .truncate()
                    .text_size(rems(0.66))
                    .text_color(theme.text_muted)
                    .child(
                        group
                            .workflow
                            .clone()
                            .unwrap_or_else(|| "Apps and statuses".into()),
                    )
            })),
        );
        let name_band = columns_strip(
            range.start,
            offset,
            h_flex().children(grid.columns()[range.clone()].iter().map(|column| {
                div()
                    .w(px(CELL_WIDTH))
                    .flex_none()
                    .h(px(NAME_BAND))
                    .px_1()
                    .pb_1()
                    .flex()
                    .items_end()
                    .child(
                        div()
                            .truncate()
                            .text_size(rems(0.7))
                            .text_color(theme.text_muted)
                            .child(column.name.clone()),
                    )
            })),
        );

        h_flex()
            .flex_none()
            .w_full()
            .h(px(GROUP_BAND + NAME_BAND))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface_raised)
            .child(
                div()
                    .w(px(ROW_HEADER_WIDTH))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_end()
                    .px_3()
                    .pb_1()
                    .text_size(rems(0.7))
                    .text_color(theme.text_subtle)
                    .child(summary),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(div().h(px(GROUP_BAND)).child(group_band))
                    .child(div().h(px(NAME_BAND)).child(name_band)),
            )
            .into_any_element()
    }

    pub(super) fn render_line(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let grid = self.grid.clone();
        let Some(line) = grid.lines().get(ix) else {
            return div().into_any_element();
        };
        match *line {
            GridLine::Stack { members } => h_flex()
                .h(px(22.))
                .pl_3()
                .text_size(rems(0.72))
                .text_color(theme.text_subtle)
                .child(format!("Stack · {members} PRs"))
                .into_any_element(),
            GridLine::Row { row } => {
                // A repository's first row is ruled off from the one before.
                let starts_repo = row
                    .checked_sub(1)
                    .and_then(|previous| grid.row(previous))
                    .is_none_or(|previous| previous.repo != grid.rows()[row].repo);
                self.render_row(&grid.rows()[row], starts_repo, &theme, cx)
            }
        }
    }

    fn render_row_header(&self, row: &GridRow, theme: &Theme) -> impl IntoElement {
        let stack_mark = row.stack.map(|place| match place {
            StackPlace::Bottom => "┗",
            StackPlace::Middle => "┃",
            StackPlace::Top => "┏",
            StackPlace::Only => "•",
        });
        h_flex()
            .w(px(ROW_HEADER_WIDTH))
            .h_full()
            .flex_none()
            .px_3()
            .gap_2()
            .border_r_1()
            .border_color(theme.border)
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
                                    .flex_none()
                                    .text_color(theme.text_subtle)
                                    .child(format!("{} #{}", row.repo, row.number.0)),
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
            )
    }

    fn render_row(
        &self,
        row: &GridRow,
        starts_repo: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Utc::now();
        let (range, offset) = self.in_view();
        let selected = self.selected.as_ref();
        let columns = self.grid.columns();
        let cells = h_flex().h_full().children(range.clone().map(|ix| {
            let at = CellRef {
                repo: row.repo.clone(),
                number: row.number,
                column: columns[ix].clone(),
            };
            let is_selected = selected == Some(&at);
            self.render_cell(row.cells[ix].as_ref(), at, is_selected, now, theme, cx)
        }));
        h_flex()
            .h(px(40.))
            .w_full()
            .border_b_1()
            .border_color(theme.border)
            .when(starts_repo, |el| el.border_t_2())
            .child(self.render_row_header(row, theme))
            .child(columns_strip(range.start, offset, cells))
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
}
