# Feature: android_core

`rostrum-ffi`: the Rust core of rostrum's Android app, exposed to Kotlin
through UniFFI. The Jetpack Compose UI calls this crate and nothing else; the
crate wraps the same gpui-free crates the desktop uses, so the two apps agree
on every rule — filtering, merge verdicts, comment anchoring, stale drafts.

## Scope

- One UniFFI object, `RostrumCore`, opened over an app-private data
  directory, holding the settings file, the SQLite cache and drafts, the
  GitHub session and the paired desktop.
- The GitHub session: a token handed in by Kotlin (pasted, or from the
  desktop), its verification, and the viewer it belongs to.
- Settings: watched repositories (typed validation), refresh cadence,
  notification toggles, and the feed's persisted filter preferences.
- The feed: cached and refreshed snapshots, per-repository load state,
  filtering, collapse, the author roster, divergence from base, and
  background merge-state re-checks delivered through an observer.
- Pull request detail: header with the merge verdict and draft action,
  the conversation with markdown flattened to blocks, threads, checks,
  labels, and every PR-level mutation (comment, reply, merge with all three
  methods, close/reopen, draft toggle, update branch by merge or rebase).
- The Files tab: overview (stats, change map, ranked files) and one file's
  diff as render-ready rows — syntax colours, word-level emphasis, comment
  anchors, inline threads and drafts.
- The pending review: drafts anchored by path/line/side (and start line),
  tagged with the head commit, persisted in SQLite, stale when the head
  moves; submission with comment/approve/request-changes.
- The paired desktop: pairing by link or by address + fingerprint, the
  local worktree status and jobs, sync-all, handoff sessions, fetching a
  fresh GitHub token, unpairing.
- Background notifications: new pull requests and new review requests since
  the last check, against a persisted seen set.
- Structured logs forwarded to Kotlin.

## Non-scope

- Rendering, navigation, confirmation dialogs, secure storage. Kotlin owns
  these; the core tells it what needs confirming (merge, close) and never
  stores a secret.
- OAuth device flow. Tokens come from pairing or pasting.
- Scheduling. Kotlin's WorkManager calls `check_notifications`; foreground
  polling is Kotlin's timer calling `refresh_feed`.
- Serving the desktop API — `rostrumd`. Running git — the desktop.
- Cross-compiling. The Android pipeline builds `-p rostrum-ffi` with
  cargo-ndk; this crate only guarantees it can (no C beyond bundled SQLite,
  `ring` not `aws-lc-rs`, no subprocesses).

## API at a glance

Kotlin names are camelCase; every I/O method is `suspend`.

| Area | Methods |
|---|---|
| lifecycle | `RostrumCore.open(dataDir)`, `warnings()` |
| session | `setGithubToken(token?)`, `githubStatus()`, `viewer()` |
| settings | `settings()`, `addRepo(input)`, `removeRepo(repo)`, `setRefreshInterval(s)`, `setPrsPerRepo(n)`, `setNotifications(newPrs, reviewRequests)`, `setAutostash(b)` |
| feed | `cachedFeed()`, `refreshFeed()`, `refreshRepo(repo)`, `setQuery(q)`, `setFilter(prefs)`, `toggleAuthor(login)`, `clearFilter()`, `toggleCollapsed(repo)`, `authorRoster(limit?)`, `setFeedObserver(observer?)` |
| detail | `pullDetail(repo, n)`, `cachedPullDetail(repo, n)`, `pullHeader(repo, n)`, `repositoryLabels(repo)`, `addLabel`, `removeLabel`, `addComment`, `replyToThread(repo, n, threadId, body)`, `merge(repo, n, method, title?, message?, expectedHeadSha)`, `closePullRequest`, `reopenPullRequest`, `setDraft(repo, n, draft)`, `updateBranch(repo, n, method, expectedHeadOid)` |
| files | `filesOverview(repo, n)`, `fileDiff(repo, n, fileIndex)` |
| review | `pendingReview`, `addDraft(repo, n, anchor, rangeStart?, body)`, `editDraft`, `removeDraft`, `discardDrafts`, `submitReview(repo, n, event, body, includeDrafts)` |
| remote | `parsePairingLink(uri)`, `pairWithLink(uri, deviceName)`, `probeDesktop(host, port)`, `pairManual(host, port, fingerprint, code, deviceName)`, `setRemote(endpoint, deviceToken)`, `clearRemote()`, `remoteStatus()`, `machineInfo()`, `localStatus(repo, n)`, `runLocalJob(repo, n, op, autostash)`, `abortLocal(repo, n)`, `startSyncAll(op, autostash)`, `syncAllStatus()`, `handoffs()`, `refreshGithubTokenFromDesktop()`, `unpair()` |
| notifications | `checkNotifications()`, `markNotificationsSeen()` |
| logging | top-level `installLogSink(sink, level)` |

Errors are one sealed class, `RostrumException`, whose subclasses say what to
do: `NotSignedIn`, `GitHubAuthFailed`, `GitHubRateLimited`, `MergeBlocked`,
`GitHubApi(status?, reason)`, `Network`, `UnknownPullRequest`, `DraftsStale`,
`NotPaired`, `DeviceRevoked`, `DesktopUnreachable`, `CertificateMismatch`,
`DesktopTimeout`, `IncompatibleDesktop`, `RemoteApi(code, reason)`,
`RemoteProtocol`, `InvalidRepo`, `DuplicateRepo`, `InvalidInput`, `Storage`,
`Internal`. `describe()` gives a sentence. No variant has a `message` field —
it would collide with `Throwable.message` in the generated Kotlin.

Colours: chips carry a `ColorRole` (success/warning/danger/draft/accent/
neutral) that Kotlin maps to its palette. The only literal colours are
GitHub label colours and syntax colours, both ARGB `u32`.

## Data and control flow

```
Compose UI ──suspend call──▶ UniFFI scaffolding (async_runtime = "tokio")
                                  │  future polled on the caller's thread,
                                  │  inside async-compat's Tokio context
                                  ▼
                            RostrumCore method
            ┌─────────────────────┼──────────────────────────────┐
            ▼                     ▼                              ▼
   state actor (mailbox)    network, outside the actor    blocking pool
   CoreState: config,       GitHubClient (reqwest+ring)   highlighting,
   repos, filter, viewer,   RemoteClient (pinned TLS)     markdown, diff rows
   known PRs, drafts, …           │
            │                     │ results applied back
            │◀────────────────────┘ through the actor
            ▼
   writer task (ordered) ──▶ rostrum-db (SQLite: cache, drafts, seen set)
   config writes ──────────▶ rostrum-config (config.json)
            │
            ▼
   observer task ──▶ FeedObserver.feedChanged(snapshot) in revision order
```

- **State lives in one actor.** Mutable state (`CoreState`) is owned by a
  task that runs closures sent over a channel, one at a time. Methods never
  hold state across an await: they read what they need from the actor, do
  network I/O outside it, and send the results back to be applied. No locks
  guard application state.
- **Writes are ordered.** SQLite writes are enqueued from inside the actor
  onto a single writer task, so they land in the order the state changed.
  Draft writes are acknowledged: `addDraft` returns only after the draft is
  on disk.
- **Refresh** takes a sequence number per repository from the actor, fetches
  every repository (bounded concurrency) with its batched divergence query,
  and applies each result only if no newer fetch already landed. Divergence
  answers are carried forward across refreshes by number. Repositories whose
  merge state GitHub is still computing get a background re-check (2s, 4s,
  8s; three per cycle — `rostrum_core::MergeProbeBudget`), applied through the
  actor and pushed to the observer.
- **Detail and diff** are served from the known pull request (retained even
  after it leaves the feed), the conversation (memory → SQLite → network),
  and the changed files, which are cached per head commit so a diff is
  fetched once per push.
- **Secrets.** The GitHub token and the device token arrive from Kotlin,
  live in memory in `CoreState`, and are handed to the clients. They are
  never written to SQLite or config, never logged (both token types redact
  themselves in `Debug`), and the only place they leave Rust is
  `PairingResult` / `DesktopGitHubToken`, so Kotlin can put them in the
  Keystore.

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-ffi/Cargo.toml` | Crate manifest; `cdylib` + `lib`; `uniffi-bindgen` bin; uniffi `cli` on host only | — |
| `crates/rostrum-ffi/uniffi.toml` | Kotlin binding config (`android = true`) | — |
| `crates/rostrum-ffi/src/lib.rs` | Scaffolding, module map | `RostrumCore`, `RostrumError` |
| `crates/rostrum-ffi/src/bin/uniffi-bindgen.rs` | The bindgen, same UniFFI release as the scaffolding | — |
| `crates/rostrum-ffi/src/error.rs` | The error enum | `RostrumError`, `RemoteErrorCode` |
| `crates/rostrum-ffi/src/types.rs` | Shared records and domain conversions | `UserRef`, `Chip`, `ColorRole`, `LabelView`, `Side`, `CheckState`, `MergeStatus`, … |
| `crates/rostrum-ffi/src/engine/` | The object, its actor, writer and observer tasks | `RostrumCore` |
| `crates/rostrum-ffi/src/session.rs` | Token and viewer | `GitHubStatus` |
| `crates/rostrum-ffi/src/settings.rs` | Settings screen | `Settings` |
| `crates/rostrum-ffi/src/feed/` | Feed snapshot, filter, roster, refresh | `FeedSnapshot`, `RepoSection`, `PrSummary`, `FeedObserver`, … |
| `crates/rostrum-ffi/src/detail/` | Header, timeline, actions | `PullDetail`, `PullHeader`, `TimelineEntry`, `MergeMethod`, … |
| `crates/rostrum-ffi/src/markdown.rs` | Tree → flat block list | `MdBlock`, `MdBlockKind`, `MdSpan` |
| `crates/rostrum-ffi/src/diff/` | Overview and diff rows | `FilesOverview`, `FileDiff`, `DiffRow`, `CommentAnchor`, … |
| `crates/rostrum-ffi/src/review/` | Pending review | `PendingReview`, `ReviewDraft`, `DraftAnchor`, `ReviewEvent` |
| `crates/rostrum-ffi/src/remote/` | Desktop pairing and jobs | `PairingResult`, `LocalStatus`, `JobResult`, `SyncRun`, … |
| `crates/rostrum-ffi/src/notifications.rs` | Notification check | `NotificationEvent`, `NotificationKind` |
| `crates/rostrum-ffi/src/logging.rs` | tracing → Kotlin | `LogSink`, `LogRecord`, `installLogSink` |

## Generating the Kotlin

```sh
cargo build -p rostrum-ffi
cargo run -p rostrum-ffi --bin uniffi-bindgen -- generate \
    --library target/debug/librostrum_ffi.so --language kotlin --out-dir out/
```

Library mode reads the metadata from the `.so`'s symbol table, so no build
profile may strip symbols.

## Invariants

- **No secrets persisted in Rust.** Tokens live in memory only and are
  redacted in `Debug`; config and SQLite never see them.
- **Anchors are computed in Rust only.** Every commentable diff line carries
  the anchor `DiffLine::anchor` derived from that very line (added/context →
  new line, RIGHT; removed → old line, LEFT; missing number → not
  commentable). `addDraft` accepts only anchors present in the current head's
  diff, and ranges only within one hunk on one side.
- **Drafts are never lost and never misplaced.** Each change is on disk
  before the call returns; a set is tagged with the head it was written
  against; a stale set cannot be submitted or extended.
- **One request per host.** Desktop calls go through `RemoteClient`, which
  moves to the next address only on a connect failure and never re-sends a
  request that reached a host.
- **UniFFI is pinned** at `=0.32.1` in the root `Cargo.toml`; the bindgen
  binary is built from the same pin, so the Kotlin and the scaffolding
  always match.
- **Android-buildable.** `ring` for TLS, bundled SQLite, no subprocesses.
