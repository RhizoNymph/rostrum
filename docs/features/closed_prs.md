# Feature: closed_prs

A third feed tab, **Closed**, listing the watched repositories' recently
merged and closed-without-merging pull requests as one list, newest ending
first — merged ones by when they merged, the others by when they closed —
each row saying which, when, by whom, and from which repository. Clicking a
row opens the ordinary pull request pane, which sees the pull request has
ended and offers no review, merge, close or draft controls.

## Scope

- The `Closed` variant of `FeedTab`: its place in the tab bar (third, after
  Issues), its badge, `[`/`]`, and persistence as `feed_tab: "closed"`.
- The domain model of an ended pull request (`PullEnding`, `ClosedPull`) and
  its reconciliation from GitHub's `state`, `mergedAt`, `closedAt`.
- One GraphQL read per repository (`CLOSED_PULL_REQUESTS`), its cadence, its
  cache table, and painting from the cache on a cold start.
- Ordering (`closed_order`), filtering (`FeedFilter::accepts_closed`),
  counting (`closed_count`), the author roster (`closed_roster`) and the row
  stream (`flatten_closed`) — all pure, in `rostrum-core`.
- The desktop's closed rows, keyboard navigation over them, and the pull
  request pane's handling of an ended pull request.

## Non-scope

- **The phone.** `rostrum-ffi` compiles against the new variant but does not
  offer the tab; see [Android](#android).
- **History beyond the window.** Each repository contributes its 50 most
  recently *updated* closed pull requests; there is no paging further back.
  See [The window](#the-window).
- **The repository view.** Its left pane lists open pull requests and issues
  only; a closed section there is a later, separate change.
- **Reopening, reverting or deleting the head branch** from the pane. The
  pane is read-only apart from comments and labels.
- **Notifications** for merges or closes. `Baseline` watches open pull
  requests only.
- **Who merged.** `mergedBy` is not fetched; the row shows the author.

## Data model

`crates/rostrum-core/src/closed/mod.rs`:

```rust
pub enum PullEnding {
    Merged { at: DateTime<Utc> },   // GitHub's mergedAt
    Closed { at: DateTime<Utc> },   // GitHub's closedAt, never merged
}
pub enum EndingError { StillOpen, MergedWithoutTime, ClosedWithoutTime }
impl PullEnding {
    pub fn from_parts(state: PullState, merged_at: Option<DateTime<Utc>>,
                      closed_at: Option<DateTime<Utc>>) -> Result<Self, EndingError>;
    pub fn at(self) -> DateTime<Utc>;
    pub fn is_merged(self) -> bool;
    pub fn state(self) -> PullState;
    pub fn verb(self) -> &'static str;     // "merged" | "closed"
}
pub struct ClosedPull { pub pull: PullRequest, pub ending: PullEnding }
pub struct ClosedIx(pub usize);           // into RepoState::closed, unfiltered
```

- **An ending is one of two kinds, with exactly one time.** GitHub reports
  `state` beside two nullable timestamps, and a merge carries *both* (GitHub
  closes a pull request as it merges it). Folding them into an enum makes
  "merged and closed", "ended with no time" and "open but listed as closed"
  unrepresentable; `from_parts` is the only place the three fields meet.
- **`state` decides the kind.** A stray `mergedAt` on a `CLOSED` pull request
  does not make it a merge; a `MERGED` one without `mergedAt` is rejected
  (`MergedWithoutTime`) rather than guessed at.
- **`ClosedPull` wraps the same `PullRequest` the open feed holds**, so the
  pull request pane renders a closed one unchanged. The merge fields
  (`mergeable`, `merge_state`, `checks`, `base_divergence`, `pushed_at`) are
  never fetched for a closed pull request and stay unknown; nothing on the
  Closed tab reads them.

`RepoState` (`crates/rostrum-core/src/state.rs`) gains `closed:
Vec<ClosedPull>` (fetch order) and `closed_load: LoadState`, separate from the
open lists' load states for the same reason issues have their own: separate
requests, separate failures. `closed_load` stays `Idle` until the tab is first
shown.

`FeedRow::ClosedRow { repo: RepoIx, closed: ClosedIx }` is the tab's item
row. `TabCounts` gains `closed`.

## The window

GitHub's `PullRequestOrderField` is `CREATED_AT` or `UPDATED_AT`; no
connection orders by `mergedAt` or `closedAt`. The read therefore asks for

```graphql
pullRequests(states: [MERGED, CLOSED], first: 50,
             orderBy: {field: UPDATED_AT, direction: DESC})
```

and `closed_order` sorts what arrives by ending time. Ending a pull request
updates it, so every recent merge and close is in the window. The cost of the
proxy: a pull request touched after it ended — a late comment, a label, a
bot's back-link — also counts as recently updated, and enough of those push
the *oldest* endings out of the 50. The tab is therefore "the most recent
endings, give or take its tail", never "everything that ended since X": its
newest rows are exact, and its oldest rows may have gaps. The captured
fixture shows the effect: `zed-industries/zed#55292` merged at 09:00 but was
updated at 18:25, ahead of `#65177`, merged at 17:50.

50 is `rostrum_github::CLOSED_PER_REPO`, a constant rather than a setting:
it covers days to weeks of a busy repository at one rate-limit point.

## Control flow

### Fetching

```
Store::set_tab(Closed) ──► refresh_closed_if_shown ──► for each repo where
                                                         closed_needs_fetch(..)
Store::refresh_all (poll / startup), tab == Closed ──► refresh_closed(every repo)
Store::add_repo, config file reload ──► refresh_closed_if_shown

refresh_closed(id)                        (crates/rostrum/src/sync/closed.rs)
  ├─ overlap guard: pending_closed[id]
  ├─ closed_load = Loading only if nothing is held (no blanking a shown list)
  ├─ Tokio: GitHubClient::closed_pull_requests(id, CLOSED_PER_REPO)
  │     └─ CLOSED_PULL_REQUESTS → ClosedPullsData::into_domain
  │           each node: ClosedNode::into_domain → PrNode::into_domain +
  │                      PullEnding::from_parts; a bad node → ClosedNodeError,
  │                      logged (warn, repo, error) and dropped alone
  └─ apply_closed_refresh: repo.closed = fetched (fetch order),
                           closed_load = Loaded { at } | Failed { message, at },
                           cache_closed → Db::save_closed_pulls (fire and forget)
```

- **Only while the tab is in use.** Closed pull requests are history; they
  are fetched when the tab is shown (for each repository whose list is
  missing, failed, or older than the poll interval — `closed_needs_fetch`) and
  on every poll while it stays shown. With the tab unused they cost nothing,
  and switching away and back within a poll interval does not refetch.
- **Rate limit.** One request per repository per fetch, which GitHub costs at
  one point (50 nodes, two bounded connections of ten each — the same shape
  as the open query). With the tab shown that is one extra point per
  repository per poll; `rateLimit` is logged at debug with each refresh.
- **Startup.** `open_database` reads `cache_closed_pull` with the other
  lists and `hydrate` fills `repo.closed` where nothing has arrived, so a
  window that starts on the Closed tab paints before the network answers.
  `refresh_all` after authentication then fetches, because the tab is shown.

### Ordering, filtering and the row stream

`flatten_tab(repos, filter, FeedTab::Closed)` (and `flatten_tab_in` with
any `FeedOrder`) returns `flatten_closed(repos, filter)`
(`crates/rostrum-core/src/closed/feed.rs`):

1. `closed_order`: every `(RepoIx, ClosedIx)` whose pull request
   `filter.accepts_closed(repo_id, closed)`, sorted by `ended_at()`
   descending, ties by repository id ascending then number descending (a merge
   queue lands several in one second; the order must not depend on which
   repository answered first).
2. One `ClosedRow` per entry, then a `Spacer` if any.
3. Per repository, in settings order: `RepoError` if its last closed fetch
   failed (even with a stale list — there is no header to carry an error
   chip), `RepoLoading` if it holds nothing and a fetch is pending or not yet
   started; each followed by a `Spacer`.

`Feed::chrome` treats consecutive `ClosedRow`s as one run whatever their
repositories, so the list draws as one container; notices are their own.

**What applies on this tab and what does not:**

| Control | Closed tab |
|---|---|
| Search | Applies — title, number, author, label, and also `owner/name` (no headers to scan) |
| Author filter, "include involved in" | Applies (author; assignee or requested reviewer when widened); the popover lists closed pull request authors (`closed_roster`, ordered by latest ending) |
| Hide drafts | Does not apply (a closed draft is history), and the button is hidden |
| Sort | Does not apply — the tab is always newest ending first; the Sort button is hidden |
| Hide empty repositories | Does not apply (no repository containers); the checkbox is hidden |
| Collapse (`c`, header click) | Does not apply; `c` is inert on this tab so it cannot fold a repository on the other tabs unseen |
| Arrange PRs, New issue | Not offered |
| "N of M shown" | Counts held closed pull requests the filter accepts, of all held |

### The tab bar, keys and badge

`FeedTab::ALL` is `[PullRequests, Issues, Closed]`. `]`/`[` move right/left
without wrapping, exactly as before; `]` on Issues now reaches Closed. The
badge is `TabCounts::closed` — `closed_count(repos, filter)`, the held closed
pull requests the filter accepts, collapse ignored. It counts the window, not
every pull request ever closed, and reads `0` until the tab has been shown
once (or the cache had a list).

### Selecting a row

`selection_for` maps a `ClosedRow` to `Selection::PullRequest { repo, number
}` — the same selection an open row makes, so the workspace opens the same
`PrDetail`. `nav::selected_row` looks a pull request selection up in both
lists and matches whichever row kind the feed holds; a pull request can be in
both for a poll after it merges, and each tab then highlights its own row.

### The pane for an ended pull request

`PrDetail::pull` falls back to the Closed tab's entry when the open list does
not hold the number (then to a link's lookup). `PrDetail::ended` combines the
two sources in `detail::ended::ended(listed, state)`:

- the conversation's `state` (fetched when the pane opens, so fresher) wins:
  `OPEN` means not ended (reopened since the list was fetched);
- `MERGED`/`CLOSED` that agrees with the list entry takes the entry's time;
  otherwise the kind stands alone (merged while the pane was open, or opened
  from a link);
- before the conversation lands, the list entry decides.

When ended, the header shows a "merged 3h ago" / "closed 3h ago" chip and the
action bar becomes a statement of how it ended, the composer and a Comment
button — no Approve, Request changes, Merge, draft toggle, Close, branch sync
or local-clone row. Labels stay editable; the Conversation, Files and Checks
tabs read as for an open pull request.

## Invariants

- **An ended pull request has exactly one ending, with a time.** Enforced by
  `PullEnding`; inconsistent nodes are rejected per node, never per list.
- **The Closed tab's order is end time, newest first, across repositories**,
  for every filter and every `FeedOrder`; ties are deterministic.
- **The tab never shows open pull requests, and the open tabs never show
  closed ones.** A stream holds one kind of item row.
- **`ClosedIx` indexes the unfiltered `RepoState::closed`**, like `PrIx`.
- **Closed fetching costs nothing while the tab is unused.**
- **A background refresh never blanks a shown list**: `Loading` is set only
  when nothing is held.
- **The pane never offers state-changing review or merge controls for an
  ended pull request.**
- **The phone never shows a Closed tab it cannot render**: `rostrum-ffi` maps
  the core's `Closed` to its Pull requests tab (see below).

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-core/src/closed/mod.rs` | Ending model, ordering, filter, count, staleness, roster, lookup | `PullEnding`, `EndingError`, `ClosedPull`, `ClosedIx`, `FeedFilter::accepts_closed`, `closed_order`, `closed_count`, `closed_needs_fetch`, `closed_roster`, `find_closed` |
| `crates/rostrum-core/src/closed/feed.rs` | The tab's row stream | `flatten_closed` (crate) |
| `crates/rostrum-core/src/closed/tests.rs` | Ending, ordering, filtering, rows, chrome, counts, staleness, roster |  |
| `crates/rostrum-core/src/feed.rs` | `FeedRow::ClosedRow`, closed-run chrome, dispatch to `flatten_closed` | `FeedRow`, `Feed` |
| `crates/rostrum-core/src/tabs.rs` | `FeedTab::Closed`, `TabCounts::closed` | `FeedTab`, `TabCounts`, `tab_counts` |
| `crates/rostrum-core/src/state.rs` | `RepoState::closed`, `closed_load` | `RepoState` |
| `crates/rostrum-github/src/closed/wire.rs` | The document and its wire types | `CLOSED_PULL_REQUESTS`, `ClosedPullsData`, `ClosedNode`, `ClosedNodeError`, `DecodedClosed` |
| `crates/rostrum-github/src/closed/mod.rs` | `GitHubClient::closed_pull_requests` | `CLOSED_PER_REPO`, `RepoClosedPulls` |
| `crates/rostrum-github/fixtures/closed/closed_zed.json` | Live capture: merges, closes, a closed draft, forks, late updates |  |
| `crates/rostrum-db/src/closed.rs` | `cache_closed_pull` table access | `Db::save_closed_pulls`, `Db::load_closed_pulls` |
| `crates/rostrum-db/src/schema.rs` | `cache_closed_pull` (added with `IF NOT EXISTS`, no version bump) |  |
| `crates/rostrum/src/sync/closed.rs` | Fetch cadence, overlap guard, apply, cache | `Store::refresh_closed`, `Store::refresh_closed_if_shown` |
| `crates/rostrum/src/sync/mod.rs` | Hydrating from the cache, the poll hook, the roster per tab | |
| `crates/rostrum/src/feed/closed.rs` | Closed row rendering | `closed_row_content` |
| `crates/rostrum/src/feed/mod.rs`, `feed/rows.rs` | Tab bar, controls hidden on the tab, `selection_for`, notices naming their repository | |
| `crates/rostrum/src/nav.rs` | Selection ↔ closed row | `selected_row` |
| `crates/rostrum/src/detail/ended.rs` | Ended state and its action bar | `Ended`, `ended` |
| `crates/rostrum/src/detail.rs` | Pull lookup falls back to the closed list; header chip | |
| `crates/rostrum-ffi/src/feed/types.rs`, `feed/state.rs`, `feed/mod.rs` | The phone's tab set excludes Closed | `FeedTab` (FFI) |

## Android

The phone's `FeedTab` (UniFFI) keeps two variants: adding one would break
the Kotlin `when` expressions over it (`SortMappings.kt`,
`FakeFeedAssembler.kt`, `FakeRostrumBackend.kt`) before the app has anything
to show. In `rostrum-ffi`, `From<rostrum_core::FeedTab>` maps `Closed` to
`PullRequests`, and `FeedState::snapshot` and `author_roster` match on that
phone tab (`PhoneTab`), so a hand-edited phone config saying `"closed"`
shows the pull requests. `feed_tab` is not a shared key, so the desktop's
choice never reaches the phone.

To offer the tab on Android:

1. `rostrum-ffi`: add `Closed` to the FFI `FeedTab` and `TabCounts.closed`;
   fetch with `GitHubClient::closed_pull_requests` on tab entry and on the
   poll while shown (mirror `refresh_closed_if_shown`, using
   `closed_needs_fetch`), cache with `Db::save_closed_pulls`/
   `load_closed_pulls`, and add a `RepoBody`-independent snapshot shape for
   one cross-repository list — e.g. `FeedBody::Closed { rows: Vec<ClosedSummary> }`
   with `ClosedSummary { repo, number, title, author, ending: Merged|Closed,
   ended_at, additions, deletions, labels }` built from `flatten_closed`'s
   rows, plus per-repository failure/loading notices.
2. `pull_detail` must find a closed pull request (it already remembers pull
   requests it has seen in `known`; seed it from the closed list) and report
   `PullState::Merged`/`Closed` so the detail screen hides merge and review.
3. Kotlin: a third tab in the feed's tab row, the `when` branches above, a
   closed row composable (ending chip + relative time + repository), and no
   sort sheet, hide-drafts or collapse on that tab.
