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
- Filter of PRs and issues within a repo, and applying the feed's sorts
  while flattening either tab. The sort model itself is `feed_sort`; see
  `docs/features/feed_sort.md`.
- The **Pull requests | Issues** tab bar: which list the stream is built from,
  per-tab counts, and `[`/`]`. What lies behind the Issues tab — fetching,
  the issue pane, creation — belongs to `issues`; see
  `docs/features/issues.md`.

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
pub struct IssueIx(pub usize);

pub enum FeedRow {
    RepoHeader { repo: RepoIx },
    StackHeader{ repo: RepoIx, stack: StackIx },          // above a stack's members
    PrRow      { repo: RepoIx, pr: PrIx, stack: Option<StackSlot> },
    IssueRow   { repo: RepoIx, issue: IssueIx },             // Issues tab only
    RepoEmpty  { repo: RepoIx },   // repo loaded, zero open PRs
    RepoError  { repo: RepoIx },   // last refresh failed
    RepoLoading{ repo: RepoIx },   // first load in flight
    Spacer     { repo: RepoIx },   // gap below a repo's container
}

pub fn flatten(repos: &[RepoState], filter: &FeedFilter) -> Feed;
pub fn flatten_in(repos: &[RepoState], filter: &FeedFilter, order: FeedOrder) -> Feed;
pub fn flatten_tab(repos: &[RepoState], filter: &FeedFilter, tab: FeedTab) -> Feed;
pub fn flatten_tab_in(repos: &[RepoState], filter: &FeedFilter, tab: FeedTab, order: FeedOrder) -> Feed;
pub fn repo_pull_rows(repo: &RepoState, items: Sort<ItemSortKey>) -> (Vec<FeedRow>, Vec<FeedStack>);
```

`flatten` walks repositories in `filter.sort.repos` order and each
repository's filtered pull requests in `filter.sort.items` order.
`flatten_in(.., FeedOrder::AsListed)` keeps the order of `repos` and of each
`prs` instead; the Android core uses it. Either way `RepoIx`/`PrIx` stay
positional, so sorting moves rows without changing what they point at.

Within a repository, pull requests are grouped into units before rows are
emitted: GitHub's stacks and detected chains become one `StackHeader` row
followed by the stack's visible members bottom first, each `PrRow` carrying a
`StackSlot` (which stack, and Bottom/Middle/Top/Only for the chain glyph).
A stack sorts as one unit. `Feed::stack(StackIx)` resolves a header to its
group. This happens on the Pull requests tab only: the Issues tab's rows are
one `IssueRow` per issue, never grouped. `repo_pull_rows` is the same layout
for a single repository, unfiltered, for the repository view. See
`stacks.md`.

`flatten` is a pure function over application state. It is the single place that
decides row order and composition, and it is unit-tested directly without a
window: empty repos, collapsed repos, error states, filtered-to-zero repos, and
the ordering guarantees below.

## Tabs

`flatten_tab(repos, filter, tab)` builds the stream for one tab; `flatten` is
`flatten_tab(.., FeedTab::PullRequests)`. Both tabs share every rule below —
runs, chrome, hide-empty, collapse, the loading/error/empty notices — and
differ only in which list (`prs` or `issues`) and which load state (`load` or
`issues_load`) they read. A stream holds `PrRow`s or `IssueRow`s, never both,
and `Feed` remembers its tab so a tab switch always counts as a change.

The tab bar above the filter bar shows each tab's count of open items the
filter accepts (`tab_counts`, collapse ignored). The active tab is
`AppState.tab`, persisted as `feed_tab` by `Store::set_tab`. `]` and `[` switch
tabs (no wrap) in the `Feed` key context, so they are inert while typing in
the filter. The selection is kept across a switch; navigation in the new tab
starts from its end because the old selection has no row there. On the Issues
tab the drafts button is hidden (it has nothing to act on), the authors
popover lists issue authors, and every repository header offers
`+ New issue`.

## Control flow

1. `SyncEngine` updates `AppState.repos` and calls `cx.notify()`.
2. The feed view observes `AppState`. On notification it calls `flatten(..)`.
3. It diffs the new `Vec<FeedRow>` against the old and calls
   `ListState::splice(old_range, new_count)` for the changed span — replacing the
   whole vector wholesale would reset scroll position and drop measured heights.
4. `list(state, render_item)` invokes `render_item(ix, window, cx)` only for rows
   in view plus overdraw. `render_item` matches on `FeedRow` and dispatches to
   `render_repo_header`, `render_pr_row`, `render_issue_row`, etc. (in
   `feed/rows.rs`). The item renderers wrap a shared row *body* —
   `pr_row_content` / `issue_row_content`, free functions of the item and the
   theme — in the row's container chrome; the repository view wraps the same
   bodies in its own rows, so an item reads identically in both places.

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

## Opening one repository

A repository header's **name** and its **open ›** affordance (on both tabs,
beside the Issues tab's **+ New issue**), and `o` on a selected pull request
or issue, raise `FeedEvent::OpenRepo`. The workspace swaps the
left pane to that repository's own view and back; the feed entity is kept
meanwhile, so its scroll position and measured rows survive. Clicking
elsewhere on the header still collapses it — the two clicks stop propagation
so they never do both. Coming back with an item selected switches the feed to
the tab that lists it (the view shows both kinds; the feed one at a time). See
`docs/features/repo_view.md`.

## Managing repositories

Repositories are added and removed from the feed's **repos** popover, which writes
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

## Merge status on a row

A PR row carries a merge chip driven by `MergeStatus::chip()` —
`conflict`, `behind`, or `blocked`, coloured by `Theme::merge_color` and
explaining itself on hover. `Draft` and `Unstable` produce no chip: the row
already carries a draft chip and a CI dot, and repeating them would be noise.
The derivation lives in `rostrum-core`; see `docs/features/pr_detail.md`.

## Distance from base on a row

`MergeStatus::Behind` says *that* a branch is behind; the row also says *by
how much*. When `pull.base_divergence` is known and `behind > 0` the row shows
`↓N` in `theme.warning`, with the base branch named in the tooltip, and the
plain `behind` chip is suppressed — it would say the same thing with less
information. When the count is unknown (cross-fork, or not yet fetched) the
existing chip stands.

The count is not part of the feed query. `Ref.compare` takes the head ref
name as an argument and a GraphQL field cannot read a sibling's value, so it
is a second, batched request per repository issued from `apply_refresh`: one
document with an aliased `compare` per pull request, branch names passed as
variables so no user data reaches the document text. Verified live at cost 1.
The result is written onto `PullRequest::base_divergence`, which
`apply_refresh` carries forward across the wholesale `repo.prs` replacement so
the chip does not flicker to absent for the round-trip.

## Sync all worktrees

When any repository has a clone configured, the header gains a row of three
buttons — `Pull all`, `Merge base into all`, `Rebase all onto base` — the
`Stash local changes` checkbox (the same `autostash` setting the detail pane
uses), and a progress line.

`Store::sync_all(kind)` enumerates every open pull request in every repository
with a clone, synchronously, into `LocalJob`s, then runs them **one at a
time** inside one retained task: git operations on one clone share refs, and
one-at-a-time is what the progress line reads. Each job is the same
`rostrum_local::run_local_job` the detail pane's buttons call — find the worktree,
run, hand off or abort a conflict — so the two cannot drift. A pull request
whose branch is not checked out anywhere is skipped with `NotCheckedOut`.
Dropping the task cancels between jobs, never mid-git.

Results stay on `Store` until the next sync replaces them, and each row shows
a chip for its outcome when it is not plain success: `handed off` (accent),
`conflict` / `failed` (danger), `refused` (warning), with the detail in the
tooltip. The progress line reads `Pull all: 3/12…` while running and
`Pull all: 9 updated, 2 handed off, 1 conflicts` afterwards, zero buckets
omitted.

Nothing is pushed. After a local merge or rebase the detail pane's "ahead"
count is the cue.

## Header popovers

`repos`, `authors` and `Sort` each open a `rostrum_ui::Popover` (the Sort
popover is described in `docs/features/feed_sort.md`). The panel's top-left
corner sits on the centre of its button, and it floats over the feed instead of
pushing it down. At most one is open (`FeedView::popover:
Option<HeaderPopover>`), so opening one closes the others. A press outside the
panel and its button closes it. `escape` closes an open popover before it clears
the filter or moves focus, because `dismiss_filter` checks the popover first.
The popovers add no key contexts: `j`/`k` still resolve in `Feed`, and typing in
the repo input still resolves in the input, so the navigation rules below are
unchanged.

## Filtering and navigation

The filter bar writes into `AppState.filter`, which `flatten` already consults —
filter state has exactly one home, and the existing store-changed path rebuilds
rows live as the query is typed.

The bar holds, top to bottom: the search box with the `drafts`, `authors`,
`repos` and `Sort` buttons; the `hide empty repos` checkbox; the sync-all row when any repo has a
clone; and the author row — a chip per person with open work, the viewer first
and everyone else by recency, with the `include involved in` checkbox beneath
it. The author row and its ordering, capping and persistence rules belong to
`author_filter`; see `docs/features/author_filter.md`.

Everything in the bar except the search query is a *preference* and is written
to the config as it changes, through `Store::edit_filter`. `FeedView::update_filter`
mutates `AppState.filter` without persisting and is now reserved for the query
alone.

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
- **A stack's rows are contiguous inside its repository's run**: its
  `StackHeader`, then its visible members bottom first. A pull request is in
  at most one stack and renders once. Stack rows draw repository chrome like
  any other body row.
- **`ListState` item count always equals `feed_rows.len()`.** Any mutation of
  the vector must be accompanied by the corresponding `splice`. A mismatch panics
  or renders stale rows.
- **A collapsed repo contributes exactly two rows** (`RepoHeader`, `Spacer`).
- **Sorting permutes, never changes, the row set.** The same rows appear
  under every sort; only their order differs, and each repository's run stays
  contiguous.
- **Indices are positional, not identity.** `RepoIx`/`PrIx` index into
  `AppState` as of the frame they were built. They must never be stored across a
  refresh; persistent selection is stored as a `Selection` — `(RepoId,
  PrNumber)` or `(RepoId, IssueNumber)` — and resolved to indices at render
  time.
- **One kind of item per stream.** A feed built for a tab holds only that
  tab's item rows.
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
| `crates/rostrum-core/src/feed.rs` | `FeedRow` (including `StackHeader` and a member's `StackSlot`), `flatten`, `flatten_in`, `flatten_tab`, `flatten_tab_in`, `repo_pull_rows`, run-boundary computation, filter |
| `crates/rostrum-core/src/sort/` | The feed's sorts, over pull requests and issues; see `feed_sort.md` |
| `crates/rostrum-core/src/tabs.rs` | `FeedTab`, `TabCounts`, `tab_counts` |
| `crates/rostrum-core/src/state.rs` | `AppState`, `RepoState`, `Selection` |
| `crates/rostrum-core/src/stack/` | Stack model, detection, grouping; see `stacks.md` |
| `crates/rostrum/src/feed/mod.rs` | Feed view entity, tab bar, `ListState` ownership, splice logic, identity selection (`selection_for`), header popovers, `FeedEvent` |
| `crates/rostrum/src/feed/rows.rs` | Per-variant row renderers (container chrome, selection, clicks, the repository header with *open ›*, a stack member's glyph, indent and pick badge), and the row bodies `pr_row_content` / `issue_row_content` — shared with the repository view — plus `relative_time` |
| `crates/rostrum/src/feed/stacks.rs`, `feed/arrange.rs` | Stack header rows, stack confirmations, Arrange picking; see `stacks.md` |
| `crates/rostrum/src/feed/sort_menu.rs` | The Sort button and popover |
| `crates/rostrum/src/nav.rs` | Keyboard navigation over either kind of item row |
| `crates/rostrum/src/sync/mod.rs` | `fetch_divergences` (the batched compare), `sync_all` |
| `crates/rostrum/src/sync/stacks.rs` | Reading each repository's stacks after a refresh; the stack operations |
| `crates/rostrum-core/src/state.rs` | `divergence_query`, `apply_divergences` (by number), `carry_forward_divergence` — shared with the Android core |
| `crates/rostrum-local/src/jobs.rs` | `run_local_job`, one job of a sync |
