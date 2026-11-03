//! Drawing the matrix: the one header row, and each line of the list — stack
//! headings and pull request rows with their cells.
//!
//! The row-header column is fixed; the check columns to its right are drawn
//! shifted left by the view's horizontal offset and clipped, and only the
//! columns in view are drawn ([`super::hscroll::HScroll::visible`]). The
//! header row is drawn the same way above the list, so a column header and
//! its cells always share an x position.
//!
//! The columns drawn are the view's [`ColumnLayout`]: a folded workflow is
//! one summary column — counts of the row's failing, running and passing
//! checks in it — and an open one is its checks. Clicking a workflow's
//! header opens or folds it.

use std::ops::Range;

use chrono::Utc;
use gpui::{AnyElement, Context, Hsla, div, prelude::*, px, rems};
use rostrum_core::{
    StackPlace,
    ci::{
        CellRef, CheckEntry, CheckStatus, ColumnRef, GridLine, GridRow, GroupSummary, RollupState,
        ShownColumn, SlotRef, Timing,
    },
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

/// A workflow as drawn in the header: which group, the drawn columns it
/// spans, and whether it is folded into one.
struct ShownGroup {
    group: usize,
    span: Range<usize>,
    folded: bool,
}

/// "✗ 2  ● 1  ✓ 9": the summary's non-zero counts, failing first.
fn summary_text(summary: &GroupSummary) -> String {
    let rollup = summary.rollup;
    let parts: Vec<String> = [
        (rollup.failing, "✗"),
        (rollup.running, "●"),
        (rollup.passing, "✓"),
        (rollup.other, "–"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, mark)| format!("{mark} {count}"))
    .collect();
    parts.join("  ")
}

impl CiView {
    /// The drawn columns in view, with the offset they are drawn at.
    fn in_view(&self) -> (Range<usize>, f32) {
        let range = self
            .hscroll
            .visible(self.layout.len(), CELL_WIDTH, self.columns_viewport);
        (range, self.hscroll.offset())
    }

    /// Each workflow's run of drawn columns, in order.
    fn shown_groups(&self) -> Vec<ShownGroup> {
        let mut groups: Vec<ShownGroup> = Vec::new();
        let groups_of = self.grid.column_groups();
        for (ix, column) in self.layout.shown().iter().enumerate() {
            let (group, folded) = match *column {
                ShownColumn::Workflow { group } => (group, true),
                ShownColumn::Check { column } => (
                    groups_of
                        .iter()
                        .position(|g| g.columns.contains(&column))
                        .unwrap_or_default(),
                    false,
                ),
            };
            match groups.last_mut() {
                Some(last) if last.group == group => last.span.end = ix + 1,
                _ => groups.push(ShownGroup {
                    group,
                    span: ix..ix + 1,
                    folded,
                }),
            }
        }
        groups
    }

    /// The one header row: a corner summarising the grid, workflow names
    /// over their jobs, then each job's name.
    pub(super) fn render_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let grid = &self.grid;
        let (range, offset) = self.in_view();
        let groups: Vec<ShownGroup> = self
            .shown_groups()
            .into_iter()
            .filter(|g| g.span.start < range.end && g.span.end > range.start)
            .collect();
        let first_group = groups.first().map_or(0, |g| g.span.start);
        let workflows = grid.column_groups().len();
        let mut summary = format!(
            "{} pull requests · {} checks in {workflows} workflows",
            grid.rows().len(),
            grid.columns().len()
        );
        if grid.hidden() > 0 {
            summary.push_str(&format!(" · {} hidden", grid.hidden()));
        }

        let group_band = columns_strip(
            first_group,
            offset,
            h_flex().children(groups.iter().map(|shown| {
                let group = &grid.column_groups()[shown.group];
                let width = CELL_WIDTH * shown.span.len() as f32;
                let workflow = group.workflow.clone();
                let name = workflow
                    .clone()
                    .unwrap_or_else(|| "Apps and statuses".into());
                let mark = if shown.folded { "▸" } else { "▾" };
                div()
                    .id(("ci-workflow", shown.group))
                    .w(px(width))
                    .flex_none()
                    .h(px(GROUP_BAND))
                    .px_1()
                    .border_l_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.surface))
                    .truncate()
                    .text_size(rems(0.66))
                    .text_color(theme.text_muted)
                    .child(format!("{mark} {name}"))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.toggle_workflow(&workflow, cx);
                    }))
            })),
        );
        let name_band = columns_strip(
            range.start,
            offset,
            h_flex().children(self.layout.shown()[range.clone()].iter().map(|column| {
                let label = match *column {
                    ShownColumn::Workflow { group } => {
                        let count = grid.column_groups()[group].columns.len();
                        if count == 1 {
                            "1 check".to_string()
                        } else {
                            format!("{count} checks")
                        }
                    }
                    ShownColumn::Check { column } => grid.columns()[column].name.clone(),
                };
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
                            .child(label),
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
        let grid = self.grid.clone();
        let cells = h_flex().h_full().children(range.clone().filter_map(|ix| {
            let slot = SlotRef {
                repo: row.repo.clone(),
                number: row.number,
                column: self.layout.column_ref(&grid, ix)?,
            };
            let is_selected = self.selected.as_ref() == Some(&slot);
            Some(match self.layout.shown()[ix] {
                ShownColumn::Workflow { group } => {
                    self.render_summary(row, group, slot, is_selected, theme, cx)
                }
                ShownColumn::Check { column } => {
                    let at = slot.cell()?;
                    self.render_cell(row.cells[column].as_ref(), at, is_selected, now, theme, cx)
                }
            })
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
            .on_click({
                let slot = SlotRef {
                    repo: at.repo.clone(),
                    number: at.number,
                    column: ColumnRef::Check(at.column.clone()),
                };
                cx.listener(move |this, event: &gpui::ClickEvent, _window, cx| {
                    this.select(slot.clone(), cx);
                    if event.click_count() >= 2 {
                        this.open_log(cx);
                    }
                })
            });

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

impl CiView {
    /// A folded workflow's cell: the row's checks in it, counted, tinted by
    /// the worst of them; the hover names what is failing or running.
    fn render_summary(
        &self,
        row: &GridRow,
        group: usize,
        slot: SlotRef,
        selected: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = &self.grid.column_groups()[group];
        let summary = row.summary(group);
        let state = summary.rollup.state();
        let color = rollup_color(state, theme);
        let id = gpui::ElementId::Name(
            format!(
                "ci-{}-{}-wf-{}",
                slot.repo,
                slot.number.0,
                group.workflow.as_deref().unwrap_or("")
            )
            .into(),
        );
        let attention: Vec<String> = row
            .attention(group)
            .map(|entry| format!("{} — {}", entry.key.name, entry.status.label()))
            .collect();
        let mut tooltip = summary.rollup.describe();
        if summary.not_run > 0 {
            tooltip.push_str(&format!(" · {} not run", summary.not_run));
        }
        if !attention.is_empty() {
            tooltip.push('\n');
            tooltip.push_str(&attention.join("\n"));
        }
        tooltip.push_str("\nspace or double-click opens the workflow");

        let tile = div()
            .size_full()
            .px_1p5()
            .rounded(px(4.))
            .border_1()
            .border_color(if selected { theme.accent } else { theme.border })
            .when(selected, |el| el.border_2());
        let tile = if matches!(state, RollupState::Empty) {
            tile.flex()
                .items_center()
                .text_size(rems(0.66))
                .text_color(theme.text_subtle)
                .child("not run")
        } else {
            tile.bg(Hsla { a: 0.14, ..color })
                .child(
                    div()
                        .truncate()
                        .text_size(rems(0.7))
                        .text_color(color)
                        .child(summary_text(&summary)),
                )
                .when(summary.not_run > 0, |el| {
                    el.child(
                        div()
                            .truncate()
                            .text_size(rems(0.64))
                            .text_color(theme.text_muted)
                            .child(format!("{} not run", summary.not_run)),
                    )
                })
        };

        div()
            .id(id)
            .w(px(CELL_WIDTH))
            .h_full()
            .flex_none()
            .p_0p5()
            .cursor_pointer()
            .tooltip(move |_window, cx| {
                cx.new(|_| TextTooltip {
                    text: tooltip.clone().into(),
                })
                .into()
            })
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, _window, cx| {
                    this.select(slot.clone(), cx);
                    if event.click_count() >= 2 {
                        this.open_selected(cx);
                    }
                }),
            )
            .child(tile)
            .into_any_element()
    }
}
