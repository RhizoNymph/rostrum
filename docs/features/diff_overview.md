# Feature: diff_overview

A visual overview of a pull request's diff, answering "what's going on and
where is the bulk of the change?" before (or instead of) reading every line.

## Scope

- A `Diff | Overview` switcher at the top of the Files tab.
- A **summary strip**: file count, total `+/−`, and added/removed/renamed
  file counts.
- The **change map**: a proportional map of the diff. One column per
  directory, its width the directory's share of total churn; within a column,
  one tile per file, its height the file's share of the directory's churn.
  Tile colour blends the theme's added-green and removed-red by the file's own
  addition/deletion mix; opacity scales with churn relative to the largest
  file, so the hot spots pop.
- The **largest changes** list: every file ranked by churn, with a stacked
  green/red bar proportional to the largest file in the diff.
- Click-through: clicking a file tile or a ranked row expands the file if
  collapsed, switches to the Diff view, and scrolls its header to the top.

## Non-scope

- No new data fetching. The overview is a pure function of the `DiffFile`s the
  Files tab already loads.
- Not a treemap in the squarified sense: no pixel-measured layout. Columns and
  tiles are sized with `relative()` fractions, so no measurement pass or
  custom element is needed.
- Zero-churn files (pure renames, mode changes) are given a minimum weight of
  1 so they stay visible; the map is proportional, not exact, at that margin.

## Data flow

```
Loadable<Vec<DiffFile>>            (already loaded by the Files tab)
        │
        ▼  rostrum_diff::overview  (pure, tested without a window)
overview_stats(files)  → OverviewStats           totals for the summary strip
change_map(files, 8, 12) → Vec<DirGroup{share, tiles: Vec<Tile{share}}>
ranked_files(files)    → Vec<usize>              churn-descending file order
        │
        ▼  rostrum::detail::overview (rendering only)
columns get .w(relative(group.share)), tiles .h(relative(tile.share))
        │ click
        ▼
PrDetail::jump_to_file(ix): un-collapse → FilesView::Diff →
rebuild_diff_rows → files::file_header_row → ListState::scroll_to
```

Grouping is by **parent directory** (full path), root files under `(root)`.
Budgets keep the map legible: at most 8 columns (the tail folds into an
`(other)` column) and 12 tiles per column (the tail folds into a `+N more`
aggregate tile). Aggregates preserve the folded files' totals and weight, so
shares still sum to 1 and nothing visually disappears.

## Files

| File | Role |
|---|---|
| `crates/rostrum-diff/src/overview.rs` | Pure aggregation and proportional layout: `OverviewStats`, `DirGroup`, `Tile`, `change_map`, `ranked_files`, `churn` |
| `crates/rostrum/src/detail/overview.rs` | Rendering: summary strip, change map, ranked list, tile colour blend |
| `crates/rostrum/src/detail/files.rs` | `FilesView` toggle bar, `file_header_row` lookup, dispatch to overview |
| `crates/rostrum/src/detail.rs` | `FilesView` state, `set_files_view`, `jump_to_file` |

## Invariants

- Column `share`s sum to ~1 across the map; tile `share`s sum to ~1 within a
  column (float tolerance). Folding overflow into aggregates must preserve
  this and the `+/−` totals.
- Every tile with `file: Some(ix)` indexes into the same `files` slice the
  diff view renders; `jump_to_file` rebuilds the rows **before** resolving the
  header row so the scroll target and the list's item count agree.
- A zero-churn file still occupies a visible sliver (`weight = churn.max(1)`).
- The overview reads only loaded state; it renders nothing while `files` is
  not `Loadable::Loaded`.

## Constraints

- GPUI's `list` needs its parent chain sized; the toggle bar is `flex_none`
  and the body sits in `flex_1().min_h_0()`, mirroring the tab body wrapper in
  `detail.rs`.
- Tiles need `.id()` (they carry tooltips and click handlers); ids are
  namespaced `("map-tile", …)` / `("ranked-file", …)` to stay unique within
  the pane.
- Colour blending is a naive HSLA lerp between `theme.added` and
  `theme.removed`; both are theme-supplied, so the overview holds no colour
  constants of its own.
