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

## Managing repositories

Repositories are added and removed from the feed's `repos` panel, which writes
through `Config` and saves immediately. Adding accepts `owner/name` or a pasted
GitHub URL, rejects duplicates and malformed input with a message under the
input rather than silently dropping them, and starts fetching the new repo at
once. Removing also clears a selection pointing into that repo — otherwise the
detail pane would be left resolving against something that no longer exists —
and drops any in-flight request for it.

The panel is the only place every configured repository is listed. Hidden and
collapsed repositories contribute no feed rows, so without it a repository with
no open pull requests could never be removed.

## Hiding empty repositories

`FeedFilter::hide_empty_repos` defaults to **on**: a feed of a dozen
repositories is mostly empty headers most of the time. A repository is dropped
entirely — header and spacer included — when it has no *visible* pull requests,
so an active search narrows the feed to the repositories that actually match.

The rule is deliberately narrow: a repository is only hidden once it has
reached `LoadState::Loaded`. One that is still loading, or that failed, stays
visible — otherwise a broken repository silently disappears instead of showing
its error, which is the failure mode most likely to waste someone's afternoon.

`Feed::hidden_repos()` reports the count so the filter bar can say how many
disappeared rather than leaving the user wondering.

## Filtering and navigation

The filter bar writes into `AppState.filter`, which `flatten` already consults —
filter state has exactly one home, and the existing store-changed path rebuilds
rows live as the query is typed.

Visible counts are computed from `RepoState.prs`, not from feed rows. A
collapsed repo hides its rows without the filter having rejected anything, so
counting rows would misreport what the filter is doing.

Keyboard navigation (`nav.rs`) is a pure function over `&Feed`, so every edge
case is testable without a window. It walks only `PrRow`s and **does not wrap**
at either end. When there is no live row — nothing selected, or the selection
was closed, filtered out, or collapsed away — `Next`/`First` enter at the first
pull request and `Previous`/`Last` at the last.

Key contexts matter here. `Feed` is scoped to the scrolling row area only, and
the filter box is a sibling under `FilterBar`, so `j`/`k`/`c`/`g` can never fire
while typing and `escape` resolves on the filter without being stolen from the
detail pane's composers.

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
