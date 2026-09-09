//! The Files tab's visual overview: where a pull request's changes fall.
//!
//! Two views over the same diff, both built from the pure layout in
//! `rostrum_diff::overview`:
//!
//! - the **change map** — one column per directory, sized by its share of the
//!   total churn, tiled by file. Tile colour blends added-green and
//!   removed-red by the file's own mix, and intensity tracks its churn.
//! - **largest changes** — every file ranked by churn with a proportional
//!   `+/−` bar, so the long tail is scannable too.
//!
//! Clicking a file in either view jumps straight to it in the diff.

use gpui::{AnyElement, Hsla, div, prelude::*, px, relative, rems};
use rostrum_diff::{
    DiffFile, DirGroup, FileStatus, Tile,
    overview::{change_map, churn, overview_stats, ranked_files},
};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Chip, DiffStat, TextTooltip, h_flex, v_flex},
};

use crate::detail::PrDetail;

/// Column budget for the change map; the rest fold into `(other)`.
const MAX_DIRS: usize = 8;
/// Tile budget per column; the rest fold into a `+N more` tile.
const MAX_TILES: usize = 12;
/// Ranked rows shown before the list is cut with a `+N more files` note.
const MAX_RANKED: usize = 40;
/// Height of the change map.
const MAP_HEIGHT: f32 = 280.;
/// Width of the proportional bar in the ranked list.
const BAR_WIDTH: f32 = 180.;

pub fn render(detail: &PrDetail, cx: &Context<PrDetail>) -> AnyElement {
    let theme = cx.theme().clone();
    let Some(files) = detail.files.loaded() else {
        return div().into_any_element();
    };

    let stats = overview_stats(files);
    let map = change_map(files, MAX_DIRS, MAX_TILES);
    let ranked = ranked_files(files);
    let max_churn = ranked
        .first()
        .map(|&ix| churn(&files[ix]))
        .unwrap_or(0)
        .max(1);

    v_flex()
        .id("diff-overview")
        .size_full()
        .overflow_y_scroll()
        .p_3()
        .gap_3()
        .child(summary_strip(&stats, &theme))
        .child(section_title("Change map", &theme))
        .child(render_map(&map, files, max_churn, &theme, cx))
        .child(section_title("Largest changes", &theme))
        .child(ranked_list(&ranked, files, max_churn, &theme, cx))
        .into_any_element()
}

fn section_title(label: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(rems(0.72))
        .text_color(theme.text_subtle)
        .child(label)
}

fn summary_strip(stats: &rostrum_diff::OverviewStats, theme: &Theme) -> impl IntoElement {
    let clamp = |n: u64| n.min(u64::from(u32::MAX)) as u32;

    h_flex()
        .gap_2()
        .flex_wrap()
        .text_size(rems(0.78))
        .text_color(theme.text)
        .child(format!("{} files changed", stats.files))
        .child(DiffStat::new(clamp(stats.additions), clamp(stats.deletions)))
        .when(stats.added_files > 0, |el| {
            el.child(Chip::new(format!("{} added", stats.added_files)).color(theme.added))
        })
        .when(stats.removed_files > 0, |el| {
            el.child(Chip::new(format!("{} removed", stats.removed_files)).color(theme.removed))
        })
        .when(stats.renamed_files > 0, |el| {
            el.child(Chip::new(format!("{} renamed", stats.renamed_files)).color(theme.warning))
        })
}

// --- change map -------------------------------------------------------------

fn render_map(
    map: &[DirGroup],
    files: &[DiffFile],
    max_churn: u64,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .w_full()
        .h(px(MAP_HEIGHT))
        .flex_none()
        .gap(px(3.))
        .children(
            map.iter()
                .enumerate()
                .map(|(group_ix, group)| render_column(group_ix, group, files, max_churn, theme, cx)),
        )
}

fn render_column(
    group_ix: usize,
    group: &DirGroup,
    files: &[DiffFile],
    max_churn: u64,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> impl IntoElement {
    v_flex()
        .w(relative(group.share))
        .h_full()
        .min_w(px(0.))
        .gap(px(2.))
        .child(
            div()
                .flex_none()
                .overflow_hidden()
                .truncate()
                .text_size(rems(0.62))
                .text_color(theme.text_muted)
                .child(format!("{} ({})", group.label, group.file_count)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_h(px(0.))
                .gap(px(2.))
                .children(group.tiles.iter().enumerate().map(|(tile_ix, tile)| {
                    render_tile(group_ix, tile_ix, tile, files, max_churn, theme, cx)
                })),
        )
}

fn render_tile(
    group_ix: usize,
    tile_ix: usize,
    tile: &Tile,
    files: &[DiffFile],
    max_churn: u64,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> AnyElement {
    let tooltip = match tile.file.and_then(|ix| files.get(ix)) {
        Some(file) => format!("{} — +{} −{}", file.path, tile.additions, tile.deletions),
        None => format!(
            "{} — +{} −{}",
            tile.label, tile.additions, tile.deletions
        ),
    };

    let base = div()
        .id(("map-tile", group_ix * MAX_TILES.max(1) * 2 + tile_ix))
        .h(relative(tile.share))
        .w_full()
        .min_h(px(2.))
        .overflow_hidden()
        .rounded(px(3.))
        .bg(tile_color(tile, max_churn, theme))
        .px_1()
        .font_family(theme.mono_font.clone())
        .text_size(rems(0.62))
        .text_color(theme.text)
        .truncate()
        .child(tile.label.clone())
        .tooltip(move |_window, cx| {
            cx.new(|_| TextTooltip {
                text: tooltip.clone().into(),
            })
            .into()
        });

    match tile.file {
        Some(file_ix) => base
            .cursor_pointer()
            .hover(|el| el.opacity(0.85))
            .on_click(PrDetail::on_click(cx, move |this, cx| {
                this.jump_to_file(file_ix, cx)
            }))
            .into_any_element(),
        None => base.into_any_element(),
    }
}

/// Green for pure additions, red for pure deletions, blended in between;
/// stronger for the files carrying more of the diff.
fn tile_color(tile: &Tile, max_churn: u64, theme: &Theme) -> Hsla {
    let churn = tile.additions + tile.deletions;
    if churn == 0 {
        return Hsla {
            a: 0.25,
            ..theme.text_subtle
        };
    }
    let removed_ratio = tile.deletions as f32 / churn as f32;
    let mixed = mix(theme.added, theme.removed, removed_ratio);
    let intensity = (churn as f32 / max_churn as f32).sqrt();
    Hsla {
        a: 0.16 + 0.42 * intensity,
        ..mixed
    }
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0., 1.);
    Hsla {
        h: a.h + (b.h - a.h) * t,
        s: a.s + (b.s - a.s) * t,
        l: a.l + (b.l - a.l) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

// --- ranked list ------------------------------------------------------------

fn ranked_list(
    ranked: &[usize],
    files: &[DiffFile],
    max_churn: u64,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> impl IntoElement {
    let hidden = ranked.len().saturating_sub(MAX_RANKED);

    v_flex()
        .gap_0p5()
        .children(
            ranked
                .iter()
                .take(MAX_RANKED)
                .filter_map(|&ix| files.get(ix).map(|file| (ix, file)))
                .map(|(ix, file)| ranked_row(ix, file, max_churn, theme, cx)),
        )
        .when(hidden > 0, |el| {
            el.child(
                div()
                    .px_2()
                    .text_size(rems(0.7))
                    .text_color(theme.text_subtle)
                    .child(format!("+{hidden} more files")),
            )
        })
}

fn ranked_row(
    ix: usize,
    file: &DiffFile,
    max_churn: u64,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> impl IntoElement {
    let path_color = match file.status {
        FileStatus::Added => theme.added,
        FileStatus::Removed => theme.removed,
        FileStatus::Renamed | FileStatus::Copied => theme.warning,
        _ => theme.text,
    };
    let hover_bg = theme.surface_hover;

    h_flex()
        .id(("ranked-file", ix))
        .gap_2()
        .px_2()
        .py_0p5()
        .rounded(px(4.))
        .cursor_pointer()
        .hover(move |el| el.bg(hover_bg))
        .on_click(PrDetail::on_click(cx, move |this, cx| {
            this.jump_to_file(ix, cx)
        }))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .font_family(theme.mono_font.clone())
                .text_size(rems(0.72))
                .text_color(path_color)
                .child(file.path.clone()),
        )
        .child(DiffStat::new(file.additions, file.deletions))
        .child(churn_bar(file, max_churn, theme))
}

/// A stacked green/red bar whose length is the file's churn relative to the
/// largest file in the diff.
fn churn_bar(file: &DiffFile, max_churn: u64, theme: &Theme) -> impl IntoElement {
    let add = file.additions as f32 / max_churn as f32;
    let del = file.deletions as f32 / max_churn as f32;

    div()
        .flex()
        .flex_row()
        .w(px(BAR_WIDTH))
        .h(px(8.))
        .flex_none()
        .rounded(px(2.))
        .overflow_hidden()
        .bg(Hsla {
            a: 0.35,
            ..theme.surface_raised
        })
        .child(div().w(relative(add)).h_full().bg(theme.added))
        .child(div().w(relative(del)).h_full().bg(theme.removed))
}
