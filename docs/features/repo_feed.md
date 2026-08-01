# Feature: repo_feed

The primary screen: open PRs across all configured repositories, presented as a
vertical stack of per-repo containers in one continuous scroll.

## Scope

- Flattening `Vec<RepoState>` into a single renderable row stream.
- Rendering repo header rows, PR rows, and per-repo empty/error/loading rows.
- Per-repo container chrome (borders, rounding, background) derived from a row's
  position within its repo's run.
- Collapse/expand of a repo section.
- Selection state and keyboard navigation within the feed.
- Sort and filter of PRs within a repo.

## Non-scope

- Fetching PR data — see `github_sync`.
- Anything shown after a PR is selected — see `pr_detail`.
- Theme and component primitives — see `ui_foundation`.

## The core design constraint

The obvious implementation — an outer `div().overflow_y_scroll()` containing one
bordered container per repo, each holding its own list of PR rows — does not work
with GPUI's virtualized lists, and does not scale without them.

`uniform_list` and `list` compute their visible row range from **their own
bounds**. With the default `ListSizingBehavior::Auto`, a virtualized list attaches
no children to the Taffy tree, so inside an auto-height parent it collapses to
zero height and renders nothing. With `ListSizingBehavior::Infer` it reports its
full intrinsic content height, the ancestor grants exactly that, and the computed
"visible range" becomes the entire list — every row is built, measured, and
painted every frame. Virtualization is silently lost in both directions.

Giving each repo container a fixed height and its own scroll region avoids that
but produces N independent nested scrollbars, which is not the requested UX.

**Resolution: flatten everything into one row stream and render it with a single
virtualized list for the whole page.** This is what Zed does in `git_panel`,
`project_panel`, and `outline_panel` — a flat `Vec<Entry>` of an enum with
header and leaf variants, matched per row inside one list's range closure.

## Data model

`crates/rostrum-core/src/feed.rs`:

```rust
pub struct RepoIx(pub usize);
pub struct PrIx(pub usize);

pub enum FeedRow {
    RepoHeader { repo: RepoIx },
    PrRow      { repo: RepoIx, pr: PrIx },
    RepoEmpty  { repo: RepoIx },   // repo loaded, zero open PRs
    RepoError  { repo: RepoIx },   // last refresh failed
    RepoLoading{ repo: RepoIx },   // first load in flight
    Spacer     { repo: RepoIx },   // gap below a repo's container
}

pub fn flatten(repos: &[RepoState], filter: &FeedFilter) -> Vec<FeedRow>;
```

`flatten` is a pure function over application state. It is the single place that
decides row order and composition, and it is unit-tested directly without a
window: empty repos, collapsed repos, error states, filtered-to-zero repos, and
the ordering guarantees below.

## Control flow

1. `SyncEngine` updates `AppState.repos` and calls `cx.notify()`.
2. The feed view observes `AppState`. On notification it calls `flatten(..)`.
3. It diffs the new `Vec<FeedRow>` against the old and calls
   `ListState::splice(old_range, new_count)` for the changed span — replacing the
   whole vector wholesale would reset scroll position and drop measured heights.
4. `list(state, render_item)` invokes `render_item(ix, window, cx)` only for rows
   in view plus overdraw. `render_item` matches on `FeedRow` and dispatches to
   `render_repo_header`, `render_pr_row`, etc.

`list` is used rather than `uniform_list` because PR rows are variable height
(title wrapping, label chips, CI status lines). `ListState` stores items in a
`SumTree` keyed by cumulative height, so offset lookups stay O(log n).

## Container chrome without containers

Each row draws the piece of the container border that belongs to it, based on its
position within its repo's contiguous run:

| Position in run | Styling |
|---|---|
| First row (always `RepoHeader`) | top border, `rounded_t_md`, left+right borders |
| Middle rows | left+right borders only |
| Last row of the run | bottom border, `rounded_b_md`, left+right borders |
| `Spacer` | no borders, fixed vertical gap |

`flatten` records run boundaries so rendering does not have to rescan. The visual
result is indistinguishable from discrete containers, but there is exactly one
scroll region and one virtualized list.

## Invariants

- **Contiguity.** All rows belonging to one repo form an unbroken run in
  `Vec<FeedRow>`, ordered `RepoHeader`, then body rows, then `Spacer`. Nothing
  else may be interleaved. Container chrome correctness depends on this.
- **Exactly one header per repo**, and it is always the first row of the run.
- **`ListState` item count always equals `feed_rows.len()`.** Any mutation of
  the vector must be accompanied by the corresponding `splice`. A mismatch panics
  or renders stale rows.
- **A collapsed repo contributes exactly two rows** (`RepoHeader`, `Spacer`).
- **Indices are positional, not identity.** `RepoIx`/`PrIx` index into
  `AppState` as of the frame they were built. They must never be stored across a
  refresh; persistent selection is stored as `(RepoId, PrNumber)` and resolved to
  indices at render time.
- **Sticky headers are not available for free.** Zed's `sticky_items` decoration
  is implemented against `uniform_list` only. If sticky repo headers are wanted
  later, an equivalent must be written for `List`.

## Escape hatch

If total row count across all repos stays small (a few hundred), plain nested
`div`s inside one `overflow_y_scroll` container will lay out correctly and are
simpler. The flattened design is the one that survives a user adding thirty
repositories, and is what everything below assumes.

## Files

| File | Role |
|---|---|
| `crates/rostrum-core/src/feed.rs` | `FeedRow`, `flatten`, run-boundary computation, filter/sort |
| `crates/rostrum-core/src/state.rs` | `AppState`, `RepoState`, `PrSummary` |
| `crates/rostrum/src/feed/mod.rs` | Feed view entity, `ListState` ownership, splice logic |
| `crates/rostrum/src/feed/rows.rs` | Per-variant row renderers |
| `crates/rostrum/src/feed/nav.rs` | Keyboard navigation, selection actions |
