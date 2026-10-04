# Feature: feed_sort

Ordering the feed: which repository container comes first, and which item —
pull request or issue, by tab — comes first inside each one. Two independent sorts, each a key and a
direction, chosen from a **Sort** popover in the feed header and remembered in
`config.json`.

## Scope

- The sort model: keys, directions, defaults, and the rule for switching keys.
- The comparisons, as pure functions in `rostrum-core`, including the
  aggregation hook for a group of pull requests that sorts as one unit (a
  stack).
- Applying both sorts in `flatten_tab`, on both the Pull requests and the
  Issues tab: one item sort serves both lists.
- The GraphQL fields the keys need, and caching the repository-level ones.
- Persisting both sorts through `feed_filter`/`absorb_filter`.
- The desktop's Sort button and popover.

## Non-scope

- Grouping pull requests into stacks — see `stacks.md`. `flatten`'s
  `push_units` sorts each stack as one unit through `compare_groups`, with the
  stack's visible members bottom first.
- The Android app's Sort sheet: see `docs/features/android_issues_repo.md`.
  The phone receives both sorts and the core's option labels through
  `rostrum-ffi` and orders nothing itself.
- A separate sort per tab. Issues and pull requests share the one item sort;
  the tab never changes the order setting, only which list it applies to.
- Filtering. The sort never hides anything; see `author_filter` and
  `repo_feed`.

## Data model

`crates/rostrum-core/src/sort/mod.rs`:

```rust
pub enum SortDirection { Ascending, Descending }
pub enum KeyKind { Time, Text, Count }          // decides default + naming

pub trait SortKey: Copy + Eq {
    const ALL: &'static [Self];                 // menu order
    fn kind(self) -> KeyKind;
    fn label(self) -> &'static str;
    fn default_direction(self) -> SortDirection;
}

pub enum RepoSortKey { Pushed, Updated, Created, Owner, Name, Stars }
pub enum ItemSortKey { Pushed, Updated, Created, Author, Title }

pub struct Sort<K> { key: K, direction: SortDirection }   // private fields
pub struct FeedSort { pub repos: Sort<RepoSortKey>, pub items: Sort<ItemSortKey> }
pub enum FeedOrder { AsListed, Sorted(FeedSort) }
```

**Invalid combinations are unrepresentable.** Stars is a repository key only,
owner is repository-only and author item-only. Two key enums rather than one
with a validity check means no code path — the menu, the config file, a
future caller — can build "pull requests by stars". The config decoder rejects
one (and falls back, see below); the menu is built from `K::ALL`, so it can
only offer valid keys.

**Directions are named per kind.** `KeyKind::direction_label`:

| Kind | Descending | Ascending | Default |
|---|---|---|---|
| Time (pushed, updated, created) | Newest first | Oldest first | Newest first |
| Text (owner, name, author, title) | Z→A | A→Z | A→Z |
| Count (stars) | Most | Fewest | Most |

**Choosing a different key resets the direction** to that key's default
(`Sort::choose`). "Oldest first" carried from a time key onto a name key would
silently become "Z→A", which nobody chose. Choosing the current key again is
a no-op, so a stray second click is harmless. `Sort::reverse` flips it.

**Defaults:** repositories by pushed, newest first; items by created, newest
first (`FeedSort::default`).

`crates/rostrum-core/src/repo_meta.rs` — what a repository key reads:

```rust
pub struct RepoMeta {
    pub owner: RepoOwner,                 // login + OwnerKind { Organization, User }
    pub pushed_at: Option<DateTime<Utc>>, // null for a never-pushed repository
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub stars: u32,
}
```

`RepoState::meta: Option<RepoMeta>` — `None` until a refresh or the cache
supplies it. `PullRequest::pushed_at: Option<DateTime<Utc>>`.

## What each key reads

| Sort | Key | Value |
|---|---|---|
| Repos | Pushed | `RepoMeta::pushed_at` — a push to any branch |
| Repos | Updated | newest `updated_at` among the repository's open items; the repository's own `updatedAt` when it has none |
| Repos | Created | `RepoMeta::created_at` |
| Repos | Owner | `RepoMeta::owner.login`, or the owner half of the `RepoId` before metadata arrives |
| Repos | Name | the name half of the `RepoId` |
| Repos | Stars | `RepoMeta::stars` |
| Items | Pushed | `PullRequest::pushed_at`, see below; for an issue, `Issue::updated_at` |
| Items | Updated | `updated_at` — GitHub bumps it on pushes, comments and reviews |
| Items | Created | `created_at` |
| Items | Author | the author's login |
| Items | Title | the title |

Repository "updated" uses **all** open items — pull requests and issues, on
either tab — not the filtered ones, so the repository order does not shift as
a search is typed or the tab is switched.

### "Pushed" on an issue

An issue has no branch, so nothing is ever pushed to it.
`issue_sort_value(issue, ItemSortKey::Pushed)` returns the issue's
`updatedAt` instead, so "pushed" orders the Issues tab exactly as "updated"
does. The alternative — treating it as unknown — would sink every issue to
the number tie-break and make the default-looking sort look broken on that
tab. The popover's **pushed** button says so in its tooltip.

### Where "pushed" comes from for a pull request

GitHub's GraphQL API has no push time on a pull request, and
`Commit.pushedDate` has been removed. `PullRequest::pushed_at` is the later of:

- the head commit's `committedDate` (`commits(last: 1)`, already in the query
  for the CI rollup), and
- the newest `HeadRefForcePushedEvent.createdAt`
  (`timelineItems(last: 1, itemTypes: [HEAD_REF_FORCE_PUSHED_EVENT])`).

Each covers what the other misses. The commit date moves with every new
commit, amend and rebase, but it is the committer's clock: a commit made days
ago and pushed now reads as days ago, and a branch reset to an older commit
reads as that commit's age. The force-push event is GitHub's own clock at push
time, but exists only for force pushes. The combination is exact for a force
push and for an ordinary push of fresh commits.

**Caveat:** a plain (non-force) push of commits authored well before the push
still reports the commit time. The live capture in the tests shows both
shapes: `cli/cli#13894` was force-pushed 20 s after its last commit (the event
wins), `#13340` was force-pushed in May and committed to again in September
(the commit wins).

`PullRequestCommit` timeline items were considered and rejected: they carry the
same `committedDate` and no push time, so they add cost without information.

## Control flow

### Comparing

`crates/rostrum-core/src/sort/compare.rs`, all pure:

1. `repo_sort_value(repo, key)` / `item_sort_value(pr, key)` /
   `issue_sort_value(issue, key)` → `Option<SortValue>`, where `SortValue` is `Time`, `Text(TextKey)` or
   `Count`. `TextKey` is the trimmed, lower-cased form.
2. `compare_values` orders two values in a direction with **unknowns last in
   both directions**.
3. `compare_repos` breaks ties by repository name, then owner (both
   case-folded), then the exact `RepoId`. `compare_items` and
   `compare_issues` break ties by number. Tie-breaks always run ascending, whatever the direction.
4. `order_repos(repos, sort) -> Vec<RepoIx>` and
   `order_items(prs, &mut [PrIx], sort)` / `order_issues(issues, &mut
   [IssueIx], sort)` produce display order with a stable sort; the item
   orders only permute the indices they are handed (the ones the filter let
   through).

Internally the item comparisons are written once, over a private
`SortItem` trait implemented by `PullRequest` and `Issue` (its value for a key,
and its number for ties), so the two tabs cannot drift apart. The public
pull-request functions keep their concrete signatures.

### Groups (stacks)

```rust
pub fn sort_key_for_group(members: &[&PullRequest], key: ItemSortKey,
                          direction: SortDirection) -> Option<SortValue>;
pub fn compare_groups(a: &[&PullRequest], b: &[&PullRequest],
                      sort: Sort<ItemSortKey>) -> Ordering;
```

`members` runs bottom first. Text keys use the bottom member — the name a
stack is known by. Time keys use the maximum across members for a descending
sort and the minimum for an ascending one, skipping unknowns, so a stack with
fresh activity anywhere in it surfaces under "newest first". Group ties break
on the bottom members' numbers. `compare_items` is `compare_groups` over two
groups of one, so lone items and stacks cannot be ordered by different rules.

### Flattening

All four entry points end in `flatten_tab_in(repos, filter, tab, order)`:
`flatten` is the Pull requests tab sorted by `filter.sort`, `flatten_in` the
Pull requests tab in an explicit order, and `flatten_tab` either tab sorted.
`flatten_tab_in` walks repositories in `order_repos` order and, inside each,
orders the filtered `PrIx`s with `order_items` or `IssueIx`s with
`order_issues`. Indices remain positional — a `RepoIx` still indexes
`AppState.repos` — so sorting changes row order and nothing else: selection,
navigation and chrome are untouched. `FeedOrder::AsListed` skips both sorts;
the Android core uses it.

### Fetching and caching

The feed query (`OPEN_PULL_REQUESTS`) gains, on the repository,
`pushedAt createdAt updatedAt stargazerCount owner { __typename login }`, and
on each pull request `committedDate` inside the existing `commits(last: 1)`
and one `timelineItems(last: 1, itemTypes: [HEAD_REF_FORCE_PUSHED_EVENT])`.
Verified live at **cost 1** with 25 pull requests.

`crates/rostrum-github/src/graphql/sort_fields.rs` holds the wire types:
`RepoMetaNode` is `#[serde(flatten)]`ed into `RepositoryNode` with every
field optional, so an older response still decodes (as `meta: None`);
`OwnerNode` maps `Organization` to `OwnerKind::Organization` and anything else
to `User` rather than failing the repository. `RepoPullRequests::meta` carries
the result out of `GitHubClient::open_pull_requests`.

On the desktop, `Store::apply_refresh` writes `meta` onto the `RepoState`
(keeping the previous value if an answer lacks it) and caches it with
`Db::save_repo_meta` alongside the pull requests. `open_database` reads it
back with `Db::load_repo_meta`, and `hydrate` fills `meta` only where the
network has not already. Without the cache a feed sorted by pushed would open
in name order and reshuffle one repository at a time as the staggered first
refresh answers.

`cache_repo_meta` is a cache table like the others (listed in `CACHE_TABLES`,
pruned by `prune_cache`, corrupt rows dropped as misses). It is created with
`IF NOT EXISTS` on open, so no cache schema bump was needed.

The Android core copies `meta` from each fetch into its `RepoState` too, but
does not cache it.

### Persisting

`Config` gains `repo_sort` and `item_sort`:

```json
"repo_sort": { "key": "pushed", "direction": "descending" },
"item_sort": { "key": "created", "direction": "descending" }
```

`FeedFilter::sort` carries them at runtime, so they ride the existing funnel:
`feed_filter` restores them, `absorb_filter` records them, and
`Store::edit_filter` saves on every change. They live on `FeedFilter` because
they are the same kind of standing preference, but they are not a filter:

- `FeedFilter::is_active` ignores the sort;
- `FeedFilter::cleared` resets every filter and keeps the sort, and
  `Store::clear_filter` (escape, the clear button) uses it.

Decoding is forgiving in two ways. A sort with no `direction` takes its key's
default, the same rule as the menu. A sort that cannot be decoded at all — a
typo, a key from a newer build, stars on the item sort — falls back to the
default for that sort alone (`lenient_repo_sort`/`lenient_item_sort`) instead
of failing the whole file and costing the user their repository list. A config
with neither field loads with the defaults.

### The popover

`crates/rostrum/src/feed/sort_menu.rs`. A **Sort** button beside **authors**
and **repos**, labelled with both sorts — `Sort: pushed ↓ · created ↓`
(`FeedSort::summary`; times and counts as arrows, text as `A→Z`/`Z→A`). It is
the third `HeaderPopover` variant, so at most one header popover is open, and
it is anchored like the others: top-left on the button's centre.

Two sections, **Repositories** and **Pull requests & issues** — one item
sort serves both tabs, so the label names both rather than following the
active tab. Each lists `K::ALL` as
buttons, the current key `Primary`, and a direction button naming the current
direction (`Newest first ⇅`) that reverses it. Clicks call
`Store::{choose,reverse}_{repo,item}_sort`, which go through `edit_filter`,
log the new sort at debug, and notify — the feed's store observer re-flattens
and splices live.

A repository's own view follows the item sort too: both halves of its
sidebar — pull requests and issues — are listed in `sort.items` order
(`repo_view::order::ListOrder`), unfiltered. It has no sort control of its
own.

The popover adds no key context and takes no focus, so `j`/`k` still resolve
in the `Feed` context and typing still resolves in the filter box; `escape`
closes it first, like the other two.

## Invariants

1. **A sort never hides or adds rows.** `order_items` only permutes the
   filtered indices; `order_repos` is a permutation of all repositories.
2. **Contiguity survives sorting.** Each repository's rows remain one run,
   header first, spacer last.
3. **Unknown sorts last in both directions.** Reversing must not drag
   unloaded repositories to the top.
4. **Ties are deterministic and direction-independent**: name, owner, id for
   repositories; number for items.
5. **Text comparison ignores case.**
6. **A group of one orders exactly like the item.**
7. **Choosing a different key resets the direction; choosing the same key
   does not.**
8. **`feed_filter` and `absorb_filter` cover both sorts**, and an unreadable
   sort never costs the rest of the config. The feed tab (`feed_tab`) is a
   separate field in the same file; saving either leaves the other intact.
10. **Both tabs use the same item sort**, and "pushed" on an issue is its
    "updated".
9. **Clearing the filter keeps the sort.**

## Files

| File | Role |
|---|---|
| `crates/rostrum-core/src/sort/mod.rs` | `SortDirection`, `KeyKind`, `SortKey`, `RepoSortKey`, `ItemSortKey`, `Sort`, `FeedSort`, `FeedOrder` |
| `crates/rostrum-core/src/sort/compare.rs` | `SortValue`, `TextKey`, `repo_sort_value`, `item_sort_value`, `issue_sort_value`, `compare_repos`, `compare_items`, `compare_issues`, `sort_key_for_group`, `compare_groups`, `order_repos`, `order_items`, `order_issues` |
| `crates/rostrum-core/src/sort/tests.rs` | Every key both ways, defaults, resets, ties, unknowns, groups, flatten, serde |
| `crates/rostrum-core/src/sort/tests/issues.rs` | The item sort over issues, "pushed" falling back to "updated", the Issues tab under both sorts |
| `crates/rostrum-core/src/repo_meta.rs` | `RepoMeta`, `RepoOwner`, `OwnerKind` |
| `crates/rostrum-core/src/feed.rs` | `FeedFilter::{sort, cleared}`, `flatten`, `flatten_in`, `flatten_tab`, `flatten_tab_in` |
| `crates/rostrum-core/src/state.rs` | `RepoState::meta` |
| `crates/rostrum-core/src/model.rs` | `PullRequest::pushed_at` |
| `crates/rostrum-github/src/graphql.rs` | Query fields; `RepositoryNode::meta`, `PrNode::timeline_items`, `CommitNode::committed_date` |
| `crates/rostrum-github/src/graphql/sort_fields.rs` | `RepoMetaNode`, `OwnerNode`, `ForcePushNode`, `head_pushed_at`, fixture tests |
| `crates/rostrum-github/src/fixtures/feed_*.json` | Live captures of the feed query |
| `crates/rostrum-github/src/client.rs` | `RepoPullRequests::meta` |
| `crates/rostrum-db/src/repo_meta.rs` | `Db::{save_repo_meta, load_repo_meta}` |
| `crates/rostrum-config/src/lib.rs` | `repo_sort`, `item_sort`, lenient decoding (shared with `feed_tab`) |
| `crates/rostrum/src/sync/mod.rs` | Applying and caching `meta`; `Store::{choose,reverse}_{repo,item}_sort` |
| `crates/rostrum/src/feed/sort_menu.rs` | The Sort button and popover |
| `crates/rostrum-ffi/src/feed/state.rs` | `Fetched::meta`; the Android snapshot's `FeedOrder::AsListed` |
