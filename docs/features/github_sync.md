# Feature: github_sync

All network I/O: token acquisition, reads, mutations, polling, caching, and
rate-limit handling.

## Scope

- Resolving a GitHub token.
- GraphQL v4 queries for bulk PR reads.
- REST v3 calls for mutations and for diff/file fetches, and the GraphQL
  mutations for the one operation REST cannot express (draft conversion).
- The polling scheduler and its overlap guard.
- SQLite cache for cold start, offline reads, and ETag storage.
- Rate-limit accounting and backoff.
- The structured error taxonomy.

## Non-scope

- Rendering anything — this subsystem has no `gpui` dependency except the thin
  `SyncEngine` entity in `crates/rostrum/src/sync.rs`.
- Diff parsing — `rostrum-github` returns raw patch text; see `diff_review`.

## Authentication

At startup, spawn `gh auth token` as a subprocess on a background executor. On
success, hold the token in memory for the process lifetime. On failure, fall back
to `$GITHUB_TOKEN`, then `$ROSTRUM_GITHUB_TOKEN`. If all fail, the app renders an
onboarding state explaining how to run `gh auth login` rather than erroring out.

The token is never written to disk, never logged (log only its last four
characters if a diagnostic is unavoidable), and never placed in config. Required
scopes are `repo` and `read:org`; `gh`'s default OAuth token already carries them.

## Async architecture

GPUI's executor is **not** Tokio, and `reqwest` requires a Tokio reactor. Zed
solves this with `gpui_tokio`, a small crate that owns a Tokio runtime, spawns a
future onto its handle, and re-wraps the `JoinHandle` as a `gpui::Task` so that
dropping the task aborts the underlying work. We take the same git dependency and
use the same bridge.

This keeps a single concurrency model at the application level: state lives in
GPUI entities, and every async result is applied on the main thread through
`entity.update(cx, ..)`. There is no second event bus and no shared `Arc<Mutex<_>>`
state store.

The poll loop follows Zed's `auto_update` shape:

```rust
// started once, held in a field so it is not dropped
cx.spawn(async move |this, cx| {
    loop {
        this.update(cx, |this, cx| this.poll_due_repos(cx))?;
        cx.background_executor().timer(TICK).await;
    }
})
```

`poll_due_repos` checks each repo's `next_refresh_at`, and for each due repo
spawns a fetch **only if** that repo's `pending: Option<Task<()>>` is `None`.
That field is the overlap guard: a slow request can never stack up behind a fast
timer. On completion the task clears `pending`, writes results into `RepoState`,
and calls `cx.notify()`.

Refresh cadence: 60s for the feed, 20s for the currently open PR, immediate on
manual refresh or after a mutation. Repos are staggered across the tick so thirty
repositories do not fire simultaneously.

## Reads — GraphQL v4

One query per repo, returning everything the feed row needs plus enough for the
detail header, avoiding a fan-out of per-PR requests:

```graphql
repository(owner: $owner, name: $name) {
  pullRequests(states: OPEN, first: 50,
               orderBy: {field: UPDATED_AT, direction: DESC}) {
    nodes {
      id number title url isDraft createdAt updatedAt
      author { login avatarUrl }
      headRefName baseRefName
      additions deletions changedFiles
      mergeable mergeStateStatus reviewDecision
      labels(first: 10) { nodes { name color } }
      comments { totalCount }
      commits(last: 1) { nodes { commit {
        statusCheckRollup { state }
      } } }
    }
  }
}
```

The conversation timeline for a selected PR is a second, deeper query issued
lazily on selection (comments, reviews with bodies, review threads with their
comments, and timeline events).

### Merge state is computed lazily

`mergeable` and `mergeStateStatus` are not stored fields. Asking for them
*starts* the computation and returns `UNKNOWN` in the same response, so a single
query can never see the answer for a pull request GitHub has not looked at
recently. Verified live: a first query returned `UNKNOWN` for four of ten open
pull requests, and an identical query two seconds later returned real values
for all four.

`Store::probe_merge_state` therefore re-queries a repository after a refresh
that saw any `MergeStatus::Computing`, backing off 2s, 4s, 8s and giving up
after three attempts per poll cycle. The budget matters: a token that may not
read a repository's merge state sees `UNKNOWN` permanently, and without a bound
that is an endless request loop rather than a slow one.

Probes are keyed by repository in `merge_probes`, so a newer probe replaces and
cancels an older one, and `refresh_all` resets the budget because a full poll
begins a new cycle.

Neither field needs the `merge-info-preview` Accept header any more; the plain
query returns both, confirmed against the live API.

Drafts never trigger a probe. A draft reports `MergeStatus::Draft` regardless of
what GitHub is computing, so there is nothing to wait for.

Rate limiting for GraphQL is cost-based against a 5000 point/hour budget; a query
of this shape costs on the order of tens of points, so a thirty-repo feed
refreshing every 60s stays comfortably inside it. The `rateLimit { cost remaining
resetAt }` field is requested on every query and recorded.

## Mutations and diffs — REST v3

| Operation | Endpoint |
|---|---|
| Changed files + patches | `GET /repos/{o}/{r}/pulls/{n}/files` |
| Whole diff (fallback) | `GET /repos/{o}/{r}/pulls/{n}` with `Accept: application/vnd.github.v3.diff` |
| Issue comment | `POST /repos/{o}/{r}/issues/{n}/comments` |
| Submit review | `POST /repos/{o}/{r}/pulls/{n}/reviews` |
| Reply in thread | `POST /repos/{o}/{r}/pulls/{n}/comments/{id}/replies` |
| Merge | `PUT /repos/{o}/{r}/pulls/{n}/merge` |
| Close | `PATCH /repos/{o}/{r}/pulls/{n}` |
| Convert to draft | GraphQL `convertPullRequestToDraft` — no REST equivalent |
| Ready for review | GraphQL `markPullRequestReadyForReview` — no REST equivalent |
| Update from base | GraphQL `updatePullRequestBranch` — REST cannot rebase |
| Divergence counts | GraphQL `Ref.compare` — read, not a mutation |

REST responses carry ETags. Store them keyed by URL in SQLite and send
`If-None-Match`; a `304` costs no rate limit and lets the cached body stand.

`/pulls/{n}/files` is paginated at 100 per page and caps at 3000 files; patches
are omitted for very large files. Both cases are represented explicitly in the
model (`PatchAvailability::{Present, Omitted, Truncated}`) rather than as an
empty patch, so the UI can say why a diff is unavailable.

### Draft conversion is the one mutation GraphQL owns

REST accepts `draft` only when a pull request is *created*. `PATCH
/repos/{o}/{r}/pulls/{n}` ignores the field, so there is no REST route out of —
or back into — draft state. The only operations that work are a pair of GraphQL
mutations:

```graphql
mutation($id: ID!) {
  payload: convertPullRequestToDraft(input: {pullRequestId: $id}) {
    pullRequest { id isDraft }
  }
}

mutation($id: ID!) {
  payload: markPullRequestReadyForReview(input: {pullRequestId: $id}) {
    pullRequest { id isDraft }
  }
}
```

Three consequences shape the implementation:

- **There is no "set draft to X".** The two directions are distinct operations
  with distinct payload types. `DraftState` models the requested *end state* and
  selects the document; `DraftState::toggled_from(is_draft)` is how a caller
  turns a current state into a target. Modelling it as an end state rather than
  a toggle is what makes a stale `is_draft` harmless — the worst case is a
  redundant request GitHub refuses.
- **Both take a node id, not `owner/name/number`.** This is why
  `PullRequest::node_id` exists and why the feed query asks for `id`: fetching
  it with the feed makes a conversion one round trip instead of a lookup
  followed by a mutation. Pull requests cached before the field existed have no
  value for it, which is what `CACHE_SCHEMA_VERSION = "2"` drops.
- **Both payloads are aliased to `payload`**, so one wire type (`SetDraftData`)
  decodes either response. The mutations return the resulting `isDraft`, and
  `set_draft` reports a result that lands on the wrong side rather than assuming
  success. A withheld `pullRequest` — permitted when the viewer may see the
  mutation result but not the pull request — is treated as success, since the
  mutation itself did not error.

A refusal (already in the target state, or no write access) arrives as HTTP 200
with a null payload and a populated `errors[]`, which the shared GraphQL helper
turns into `GitHubError::GraphQl` carrying GitHub's own wording.

### Updating a branch from its base is GraphQL-only too

Same story as draft conversion, for the same reason. REST has
`PUT /repos/{o}/{r}/pulls/{n}/update-branch`, but it takes no `update_method`
parameter and is documented as "merging HEAD from the base branch into the pull
request branch" — merge, always. Rebase exists in the web UI and in `gh pr
update-branch --rebase`, and the only programmatic route to it is the GraphQL
mutation:

```graphql
mutation($id: ID!, $oid: GitObjectID!, $method: PullRequestBranchUpdateMethod!) {
  payload: updatePullRequestBranch(input: {
    pullRequestId: $id, expectedHeadOid: $oid, updateMethod: $method
  }) { pullRequest { headRefOid } }
}
```

`PullRequestBranchUpdateMethod` has exactly two values, `MERGE` and `REBASE`,
which is why `BranchUpdateMethod` models two variants and nothing else.

`expectedHeadOid` carries the head sha the view was rendered from. GitHub
compares it against the branch's current tip and refuses the mutation when they
differ, so a branch someone pushed to between render and click is never silently
rewritten. That is the same race guard `set_draft` gets for free from asking for
an end state, spelled explicitly here because "update from base" has no end state
to check.

### Divergence counts

`MergeStateStatus::Behind` says *that* a branch is behind. It never says by how
much, and the number is what decides whether the answer is "click update" or
"this has drifted far enough to look at by hand".

```graphql
query($owner: String!, $name: String!, $base: String!, $head: String!) {
  repository(owner: $owner, name: $name) {
    ref(qualifiedName: $base) {
      compare(headRef: $head) { aheadBy behindBy status }
    }
  }
}
```

`Ref.compare` counts both sides relative to the *head* ref, which is the
direction `rostrum_core::Divergence` fixes: `behind` is always work the pull
request has not caught up with.

Two decisions worth stating:

- **`compare` decodes as `Option`, and a null is not a failure.** A head ref the
  base repository cannot resolve — the cross-fork case — comes back as
  `"compare": null` alongside a `NOT_FOUND` error rather than failing the
  request. `GitHubClient::divergence` therefore answers `Ok(None)`, catching
  `NotFound` at that one call site rather than weakening the shared `graphql`
  helper, where every other caller genuinely wants a missing resource to be an
  error. `None` means "ask the local clone instead", which is exactly what the
  detail pane does.
- **`status` is requested but not decoded.** `ComparisonStatus` restates what the
  two counts already say, and `Divergence::relation()` derives the same verdict
  locally. Decoding it as a closed enum would let a value GitHub adds later fail
  the whole query.

`Ref.compare` takes the head ref name as an argument, and a GraphQL field
cannot reference a sibling field's value, so the count cannot be folded into
the feed query. It is instead a second request per repository, issued from
`apply_refresh`: `build_divergence_batch(n)` generates one document with an
aliased `pN: ref(...) { compare(...) }` per pull request and a matching
`$bN`/`$hN` variable pair, so branch names travel as variables and never reach
the document text. Verified live at cost 1. A `NOT_FOUND` scoped to one alias
(`path: ["repository", "pN", "compare"]`) excuses only that alias —
`unexcused_batch_errors` keeps everything else — so one cross-fork pull request
cannot blank out the batch. The single `divergence()` remains for callers that
want one answer.

### One GraphQL path for reads and writes

`GitHubClient::graphql` is the single place that posts a document and unpacks the
response. It folds the three separate ways a GraphQL call fails — a non-success
HTTP status, a 200 carrying `errors[]`, and a 200 whose `data` is null — into one
decision, and maps an all-`NOT_FOUND` error array onto `GitHubError::NotFound`
naming the resource that was addressed. The feed query, the conversation query,
and both draft mutations go through it, so none of them can drift on how a
partial failure is interpreted.

## Cache — `rostrum-db`

SQLite via `sqlx`, at `~/.local/share/rostrum/cache.db`. The crate draws a hard
line between two kinds of data, and the distinction is load-bearing:

- **Cache** (`cache_pull_request`, `cache_conversation`, `cache_http`) — copies
  of things GitHub already knows. Disposable. A schema-version mismatch drops
  and recreates these tables; corrupt JSON in a row is logged, deleted, and
  treated as a miss.
- **Drafts** (`drafts`) — review comments the user wrote that have never been
  sent anywhere. **Losing these loses their work.** They survive cache schema
  changes, they are never touched by `prune_cache`, and corrupt JSON here is a
  hard error rather than a silent discard.

The store opens the database at startup and hydrates the feed from it before the
first network round trip returns, so a cold launch paints immediately. Hydration
only ever *fills gaps* — a repo that already has data from the network is never
overwritten by the cache.

### Validating a cached diff

A pull request's diff is a pure function of its head commit, so the cache is
keyed on `head_sha` rather than an HTTP ETag. If the head has not moved, the
diff cannot have changed and no request is made at all. This is both stronger
than an ETag (no round trip to revalidate) and simpler (no 304 handling, no
per-page ETag bookkeeping across a paginated response).

`cache_http` stores it as a validator/payload pair, which is what a conditional
cache entry is regardless of whether the validator came from an HTTP header.

Config is separate and human-editable: `~/.config/rostrum/config.json` holds the
repo list, poll intervals, theme choice, and default merge method. No secrets.

## Error taxonomy

`rostrum-github` uses `thiserror`; the application layer uses `anyhow`.

```rust
pub enum GitHubError {
    NoToken,                                    // gh missing and no env fallback
    Unauthorized,                               // 401 — token invalid/expired
    Forbidden { reason: String },               // 403 — scope or SSO
    RateLimited { reset_at: DateTime<Utc> },    // primary limit exhausted
    SecondaryRateLimit { retry_after: Duration },
    NotFound { resource: String },              // repo renamed/deleted/private
    MergeConflict,                              // 405/409 on merge
    GraphQl { errors: Vec<GraphQlError> },      // 200 with errors[] populated
    Network(reqwest::Error),
    Decode { context: String, source: serde_json::Error },
}
```

Two rules that matter in practice:

- **A GraphQL 200 can still be a failure.** Partial data with a populated
  `errors[]` is common when one repo in a batch is inaccessible. Decoding must
  check `errors` before trusting `data`.
- **Errors are per-repo, not global.** One failing repository renders a
  `RepoError` row in its own container; the rest of the feed keeps working.

## Invariants

- Exactly one in-flight request per repo, enforced by `pending: Option<Task<()>>`.
- Every pull request carries its GraphQL `node_id`, so no mutation needs a lookup
  round trip to address it.
- Mutations invalidate the affected PR's cache entry and trigger an immediate
  targeted refresh; optimistic local updates are reconciled by that refresh.
- Rate-limit state is checked before issuing a poll; when exhausted, polling
  pauses until `reset_at` and the UI shows the resume time rather than failing
  silently.
- The token never reaches disk, logs, or config.

## Files

| File | Role |
|---|---|
| `crates/rostrum-github/src/auth.rs` | Token resolution chain |
| `crates/rostrum-github/src/graphql/mod.rs` | Query construction, response decoding |
| `crates/rostrum-github/src/rest/mod.rs` | Mutation and file-fetch endpoints |
| `crates/rostrum-github/src/models.rs` | Wire types and domain conversions |
| `crates/rostrum-github/src/error.rs` | `GitHubError` |
| `crates/rostrum-github/src/rate_limit.rs` | Budget accounting, backoff |
| `crates/rostrum-github/src/cache.rs` | SQLite schema, ETag storage |
| `crates/rostrum/src/sync.rs` | `SyncEngine` entity, poll loop, reconciliation |

## Testing

`rostrum-github` is written against a `GitHubApi` trait with a fake
implementation backed by recorded JSON fixtures. Every wire type has a decode
test against a real captured response, since GitHub's GraphQL nullability rules
are the most likely source of silent breakage.
