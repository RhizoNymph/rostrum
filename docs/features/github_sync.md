# Feature: github_sync

All network I/O: token acquisition, reads, mutations, polling, caching, and
rate-limit handling.

## Scope

- Resolving a GitHub token.
- GraphQL v4 queries for bulk PR reads.
- REST v3 calls for mutations and for diff/file fetches.
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
      number title url isDraft createdAt updatedAt
      author { login avatarUrl }
      headRefName baseRefName
      additions deletions changedFiles
      mergeable reviewDecision
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

REST responses carry ETags. Store them keyed by URL in SQLite and send
`If-None-Match`; a `304` costs no rate limit and lets the cached body stand.

`/pulls/{n}/files` is paginated at 100 per page and caps at 3000 files; patches
are omitted for very large files. Both cases are represented explicitly in the
model (`PatchAvailability::{Present, Omitted, Truncated}`) rather than as an
empty patch, so the UI can say why a diff is unavailable.

## Cache

SQLite via `sqlx` (`sqlite` + `tls-rustls` features), at
`~/.local/share/rostrum/cache.db`. Tables: `repos`, `pull_requests`,
`pr_timeline`, `pr_files`, `etags`, `meta`. The cache exists so the feed paints
instantly on launch and remains readable offline; it is a cache, never the source
of truth, and a schema mismatch is resolved by dropping and rebuilding it.

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
