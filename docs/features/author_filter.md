# Feature: author_filter

Narrowing the feed to the people whose work you care about, and remembering
that choice — along with the feed's other standing preferences — across
restarts.

## Scope

- The author roster: who the filter can be pointed at, and in what order.
- Selecting and unselecting authors, and the `include involved in` widening.
- Persisting every feed setting that is a *preference* rather than a
  half-finished action, and restoring it at startup.
- Fetching the involvement data (`assignees`, `reviewRequests`) and the viewer's
  own login.

## Non-scope

- The search query. It is deliberately **not** persisted; see
  [Which settings persist](#which-settings-persist).
- GitHub's full `involves:` qualifier. See
  [What "involved" means](#what-involved-means).
- Filtering by anything other than a person — labels and repositories are
  reached through the search box and the repo panel respectively.
- Sorting the feed. The roster is ordered by recency; the feed itself is not
  touched by this feature.

## Data model

`crates/rostrum-core/src/model.rs`:

```rust
/// A GitHub login folded to its comparison form.
pub struct LoginKey(String);

pub struct PullRequest {
    pub author: Option<User>,
    pub assignees: Vec<User>,        // new
    pub review_requests: Vec<User>,  // new
    // …
}

impl PullRequest {
    pub fn is_authored_by(&self, login: &LoginKey) -> bool;
    pub fn involves(&self, login: &LoginKey) -> bool;
}
```

`LoginKey` exists because GitHub logins are case-insensitive and the API echoes
back whatever casing it stored. A bare `String` would make "did this call site
remember to lowercase?" a question each of the ~six comparison points could get
wrong independently, and getting it wrong shows up as a filter that silently
matches nobody. `Deserialize` is hand-written to normalise on the way in, so the
invariant also holds for the hand-edited config file, where `"RhizoNymph"` is
what a person would naturally type.

`crates/rostrum-core/src/feed.rs`:

```rust
pub struct FeedFilter {
    pub query: String,
    pub hide_drafts: bool,
    pub hide_empty_repos: bool,
    pub authors: BTreeSet<LoginKey>,  // new — empty means everyone
    pub include_involved: bool,       // new
}
```

`authors` is a set rather than an `Option<Vec<_>>`: "nobody selected" and
"everybody shown" are the same state, and giving them two representations would
invite them to disagree. `include_involved` is inert while the set is empty,
which is what lets it be a plain checkbox rather than a third selection mode.

## Control flow

### Building the roster

`crates/rostrum-core/src/authors.rs` is pure, so every ordering rule is tested
without a window.

1. `Store::authors()` calls `roster(&state.repos, viewer, &filter.authors)`.
2. `roster` folds every loaded pull request into one entry per distinct
   `LoginKey`, accumulating `open_prs` and the newest `updated_at`.
3. The viewer and any *selected* login not already present are folded in with
   zero counts.
4. Entries sort: viewer first, then by newest pull request descending, then by
   login ascending.
5. `visible(entries, selected, limit)` caps the row for rendering and reports
   how many were dropped, for the `+N more` control.

Ordering rationale, in order of the rules:

- **The viewer leads, always, even with nothing open.** Filtering to yourself is
  the reason the control exists, and with `include_involved` on it is meaningful
  *precisely* when you have authored nothing — that is the "what is waiting on
  me" case.
- **Everyone else by recency.** The set of watched repositories is large and
  mostly other people's; the person who touched something an hour ago is the one
  you are about to look for.
- **A selected login with no open work is kept.** Its pull requests may have all
  merged since. Dropping it would leave the user holding a filter they cannot
  see and therefore cannot switch off — a feed silently hiding everything with
  no visible cause.
- **Ties break on the login.** Two refreshes carrying the same timestamps must
  not reshuffle the row under the cursor.

The same reasoning caps the row: `visible` drops only from the *unselected*
tail, never the viewer and never a selection, and preserves order so expanding
and collapsing do not move chips around.

### Fetching involvement

`crates/rostrum-github/src/graphql.rs`, in `OPEN_PULL_REQUESTS`:

```graphql
viewer { login avatarUrl }
…
assignees(first: 10) { nodes { login avatarUrl } }
reviewRequests(first: 10) {
  nodes { requestedReviewer {
    ... on User { login avatarUrl }
    ... on Bot { login avatarUrl }
    ... on Mannequin { login avatarUrl }
  } }
}
```

`requestedReviewer` is a union whose `Team` member has a `name` but no `login`.
The query asks for `login` only on the three members that have one, so a team
request decodes as a node with `login: None` and is dropped by
`AuthorNode::into_user` — rather than failing the repository's whole decode over
a reviewer the filter could never have matched anyway. `AuthorNode.login` is
`Option<String>` for exactly this reason.

`viewer` rides along on the feed query rather than being fetched once at auth
time: it costs nothing extra, and it re-resolves by itself if the token is
swapped underneath a running app. Whichever repository's response lands first
sets it; the rest agree, because it is a property of the token.

### Applying the filter

`FeedFilter::accepts` gains one step before the query match:

```rust
if self.authors.is_empty() { return true; }            // nobody selected
self.authors.iter().any(|login| if self.include_involved {
    pr.involves(login)
} else {
    pr.is_authored_by(login)
})
```

Selected authors are a **union** with each other and an **intersection** with
the other filters: selecting two people shows both their work, and the draft
toggle and search box still apply on top. Because `flatten` already hides a
repository whose visible list came out empty, filtering by author composes with
`hide_empty_repos` for free.

### What "involved" means

`include involved in` widens a selection from *opened by* to *opened by,
assigned to, or awaiting review from*. It is deliberately narrower than GitHub's
`involves:` qualifier, which also counts commenting and being mentioned. Those
are traces of having *looked at* a pull request; these three are the ones that
mean it is **waiting on you**, which is what the control is for. They are also
the three available as bounded connections on the feed query — `participants`
is unbounded and would cost a cap and a round trip per busy pull request.

## Which settings persist

`crates/rostrum/src/config.rs` owns the answer, in one pair of functions:

| Setting | Persisted | Why |
| --- | --- | --- |
| `hide_empty_repos` | yes | Standing preference about what the feed is for. |
| `hide_drafts` | yes | Same. Was previously session-only, which is the bug this feature fixes. |
| `authors` | yes | Same, and expensive to re-pick by hand. |
| `include_involved` | yes | Same. |
| `autostash` | yes | A working habit, not a per-pull-request choice. |
| `query` | **no** | A search is something you are part-way through, not a preference. Restoring one would open the app onto a narrowed feed for a reason the user no longer remembers. |

`Config::feed_filter()` builds the startup filter and `Config::absorb_filter()`
records one back. They are inverses and the only reader/writer of those fields,
so the set of settings that is *saved* cannot drift from the set that is
*restored* — which is precisely how `hide_drafts` came to be written nowhere and
forgotten on every launch.

On the write side, `Store::edit_filter` is the single funnel: it applies the
edit, absorbs the filter into the config, and saves. Every persisted toggle
(`set_hide_drafts`, `set_hide_empty_repos`, `toggle_author`,
`set_include_involved`, `clear_filter`) goes through it. `FeedView::update_filter`
remains for the query alone, and is documented as such.

`Config::load_from` / `save_to` take an explicit path so the round trip is
tested against a temporary directory. "Remembered across restarts" is a claim
about those two functions agreeing, and it should not take a restart to find out
that they do not.

## Rendering

`FeedView::render_author_filter` in `crates/rostrum/src/feed.rs` draws, above
the feed and below the `hide empty repos` row:

```
authors  [you] [alice] [bob] … [+7 more]
         [x] include involved in   opened by, assigned to, or awaiting review from
```

- Chips are `Button`s, `Primary` when selected and `Subtle` otherwise — the same
  vocabulary the `drafts` toggle already uses.
- Element ids are `author-<login key>`, unique because the roster is keyed by
  `LoginKey`.
- The row renders `None` before the first refresh answers. An empty row of a
  control that is about to populate itself is worse than no row.
- `authors_expanded` lives on the view and is **not** persisted: it is a glance
  at a long list, not a preference about what the feed shows.
- `all authors` appears only with a selection, and clears it without disturbing
  the rest of the filter. `clear` (escape) still resets everything.

## Invariants

1. **Every comparison goes through `LoginKey`.** No call site compares raw
   login strings. Enforced by `is_authored_by`/`involves` taking `&LoginKey`,
   and by `LoginKey::deserialize` normalising.
2. **An empty `authors` set filters nothing out**, and `include_involved` alone
   never activates the filter (`is_active()` must stay false).
3. **`involves` is strictly wider than `is_authored_by`.** Checking the box can
   only ever reveal pull requests, never hide one.
4. **A selected author is always drawn**, however long the roster and whether or
   not they still have open work. Both `roster` and `visible` uphold this.
5. **The roster order is stable** across refreshes that carry the same data.
6. **A config written by an older build still loads**, and arrives with the
   filter off rather than with a selection the user never made. Every new field
   is `#[serde(default)]`.
7. **`feed_filter` and `absorb_filter` cover the same fields.** Adding a
   persisted setting means touching both, and
   `every_persisted_setting_survives_a_round_trip` fails if only one is updated.

## Files

| File | Role |
| --- | --- |
| `crates/rostrum-core/src/authors.rs` | `AuthorEntry`, `roster`, `visible`, `VisibleAuthors`. The roster's ordering and capping rules, and their tests. |
| `crates/rostrum-core/src/model.rs` | `LoginKey`; `PullRequest::{assignees, review_requests, is_authored_by, involves}`. |
| `crates/rostrum-core/src/feed.rs` | `FeedFilter::{authors, include_involved, accepts_author, toggle_author}`. |
| `crates/rostrum-github/src/graphql.rs` | Query fields; `AuthorNode::into_user`, `ReviewRequestNode`; decoding into the model. |
| `crates/rostrum-github/src/client.rs` | `RepoPullRequests::viewer`. |
| `crates/rostrum/src/config.rs` | Persisted fields; `feed_filter`, `absorb_filter`, `load_from`, `save_to`. |
| `crates/rostrum/src/sync.rs` | `Store::{viewer, authors, edit_filter}` and the persisted setters. |
| `crates/rostrum/src/feed.rs` | `render_author_filter`, `AUTHOR_CHIP_LIMIT`, `author_tooltip`, the toggle handlers. |

## Cost

Two bounded connections per pull request, capped at ten entries each. At the
default 25 pull requests per repository that is 500 extra nodes per repository
per poll — well inside GraphQL's node budget, and no extra round trips. `viewer`
is free.
