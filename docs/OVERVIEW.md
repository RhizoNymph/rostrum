# Rostrum — Overview

```yaml
Overview:
  description: >
    A native Rust desktop application built on GPUI that aggregates open pull
    requests across a user-configured set of GitHub repositories into a single
    vertically scrolling feed, and provides full in-app review: reading the PR
    body and comment chain, viewing the diff with syntax highlighting, leaving
    inline line comments, submitting reviews, and merging.

  subsystems:
    ui_foundation: >
      Theme, typography, and the local component layer built directly on `gpui`.
      Owns text rendering, the hand-rolled selection/copy primitive, and the
      markdown renderer. No dependency on Zed's `ui`/`theme` crates (GPL).
    repo_feed: >
      The primary screen. Flattens all repos and their PRs into a single row
      stream rendered by one virtualized `list`, styled to look like discrete
      per-repo containers.
    pr_detail: >
      Master/detail right pane. Tabbed Conversation / Files / Checks view for a
      selected PR, including the comment composer and PR-level actions.
    diff_review: >
      Unified-diff parsing, syntax highlighting, virtualized diff rendering,
      inline comment threads, and pending-review batching.
    github_sync: >
      All network I/O. Token acquisition, GraphQL reads, REST mutations, the
      polling scheduler, the SQLite cache, and rate-limit handling.

  data_flow: >
    At startup the app resolves a GitHub token (`gh auth token`, falling back to
    $GITHUB_TOKEN) and loads config + cached state from SQLite, so the feed
    paints before any network round-trip completes.

    `SyncEngine` (a GPUI entity) then runs a poll loop: one GraphQL query per
    configured repo, staggered, guarded against overlap by an in-flight `Task`
    handle. Network futures execute on a Tokio runtime bridged into GPUI's
    executor; results are applied back on the main thread via
    `entity.update(cx, ..)` followed by `cx.notify()`.

    `AppState` holds the canonical `Vec<RepoState>`. Whenever it changes, the
    feed's flat `Vec<FeedRow>` is rebuilt and pushed into `ListState` via
    `splice`, which is what actually drives re-render of the scrolling feed.

    Selecting a PR creates a `PrDetail` entity, which lazily fetches the
    conversation timeline and, on first visit to the Files tab, the changed-file
    patches. Patches are parsed into `DiffRow`s carrying old/new line numbers;
    those line numbers are what inline comments are anchored to when submitted.

    Mutations (comment, review, merge) go out over REST, are applied optimistically
    to local state where safe, and are reconciled by the next poll.

Features Index:
  ui_foundation:
    description: Theme, components, text rendering, selection, markdown.
    entry_points: [crates/rostrum-ui/src/lib.rs, crates/rostrum-md/src/lib.rs]
    depends_on: []
    doc: docs/features/ui_foundation.md
  repo_feed:
    description: Flattened, virtualized multi-repo PR feed.
    entry_points: [crates/rostrum/src/feed/mod.rs, crates/rostrum-core/src/feed.rs]
    depends_on: [ui_foundation, github_sync]
    doc: docs/features/repo_feed.md
  pr_detail:
    description: Conversation timeline, composer, and PR-level actions.
    entry_points: [crates/rostrum/src/detail/mod.rs]
    depends_on: [ui_foundation, github_sync]
    doc: docs/features/pr_detail.md
  diff_review:
    description: Diff parsing, highlighting, inline comments, review batching.
    entry_points: [crates/rostrum/src/detail/files.rs, crates/rostrum-diff/src/lib.rs]
    depends_on: [ui_foundation, github_sync, pr_detail]
    doc: docs/features/diff_review.md
  github_sync:
    description: Auth, GraphQL/REST client, polling, cache, rate limits.
    entry_points: [crates/rostrum-github/src/lib.rs, crates/rostrum/src/sync.rs]
    depends_on: []
    doc: docs/features/github_sync.md
```

## Workspace layout

Non-UI logic lives in crates that do not depend on `gpui`, so the bug-prone parts
(diff line mapping, GraphQL decoding, feed flattening) are testable with plain
`cargo test` and no window.

| Crate | gpui? | Responsibility |
|---|---|---|
| `rostrum-core` | no | Domain types, feed flattening, selection state machine |
| `rostrum-github` | no | GraphQL reads, REST mutations, auth, rate limiting, errors |
| `rostrum-diff` | no | Unified-diff parsing, `DiffRow` model, syntax highlighting |
| `rostrum-md` | no | `pulldown-cmark` → renderable markdown model |
| `rostrum-ui` | yes | Theme, components, text/selection, markdown element |
| `rostrum` | yes | Bootstrap, window, root views, `SyncEngine` |

## Foundational decisions

| Decision | Choice | Rationale |
|---|---|---|
| Auth | `gh auth token`, `$GITHUB_TOKEN` fallback | No secret storage of our own; `gh` handles SSO and refresh |
| Reads | GraphQL v4 | One round-trip per repo instead of dozens; cost-based rate limit |
| Mutations | REST v3 | Simpler, better-documented endpoints for merge/review/comment |
| UI deps | `gpui` + `gpui_platform` only | Zed's `ui`/`theme`/`syntax_theme` are GPL-3.0-or-later |
| Diff parsing | hand-rolled | `diffy` requires `---`/`+++` headers GitHub's per-file patches lack, and exposes neither `\ No newline` nor the raw `@@` line |
| Highlighting | `syntect` (pure-Rust regex) | One dependency covering many languages, versus matching the tree-sitter ABI across a grammar crate per language. Tree-sitter remains the better long-term choice |
| Cache | SQLite via `sqlx` | Instant cold start, offline reads, ETag storage |
| Async | Tokio bridged into GPUI's executor | GPUI's executor is not Tokio; `reqwest` requires a Tokio reactor |

## Hard constraints

- **Rust edition 2024.** GPUI's `spawn` family takes native async closures
  (`cx.spawn(async move |this, cx| ...)`). Older two-layer-closure examples found
  online will not compile.
- **Pinned git dependency, all from one rev.** `gpui_platform` is not published,
  and it declares `gpui` as a bare path dep with no version. Declaring
  `gpui = "0.2.2"` from crates.io alongside a git `gpui_platform` compiles two
  incompatible copies of `gpui`. Pin `gpui`, `gpui_platform`, and `gpui_tokio` to
  the same rev.
- **`cx.notify()` is always manual.** GPUI does not dirty-check entity fields.
  Every mutation path that should repaint must end in `cx.notify()`.
- **Subscriptions must be retained.** `cx.observe`/`cx.subscribe` return a
  `Subscription` that unsubscribes on drop. Store them in a `Vec<Subscription>`
  field or `.detach()` them.
- **`Task` cancels on drop.** Long-lived tasks (poll loops) must be held in a
  field, or they die immediately.
- **`.id()` is required before `.on_click()` or `.overflow_y_scroll()`.** Those
  live on `StatefulInteractiveElement`, which only exists for elements given a
  stable `ElementId`; GPUI needs that identity to persist per-frame state.

## Linux build prerequisites

GPUI needs system libraries beyond the Rust toolchain. Zed's `script/linux`
installs them; the relevant set includes `libasound2-dev`, `libfontconfig-dev`,
`libwayland-dev`, `libxkbcommon-x11-dev`, `libssl-dev`, `libzstd-dev`,
`libvulkan1`, and `mesa-vulkan-drivers`.

**Currently built Wayland-only.** The `x11` feature needs `libxkbcommon-x11-dev`,
which is not installed on this machine (the runtime `.so.0` is present but the
development symlink is not). To enable X11:

```sh
sudo apt install libxkbcommon-x11-dev
```

then add `"x11"` back to the `gpui`/`gpui_platform` feature lists in the root
`Cargo.toml`. GPUI picks whichever backend it finds at runtime.

## Dependency sourcing

The workspace currently points `gpui`, `gpui_platform`, and `gpui_tokio` at a
local Zed checkout (`/home/nymph/Code/devtools/zed`) for fast iteration — Zed's
git history is ~500 MB, so a cargo git dependency would clone that on first
build. The portable form (a pinned git rev on all three) is documented inline in
the root `Cargo.toml`.

Zed patches `async-process`, `async-task`, and `calloop` to its own forks. The
root `Cargo.toml` replicates those `[patch.crates-io]` entries, without which the
dependency graph does not resolve outside Zed's workspace.

## Phasing

1. **Shell** ✅ — window, theme, config, token, GraphQL client, feed with
   flattened list, selection, detail header.
2. **Conversation** ✅ — timeline, markdown renderer, comment composer, thread
   replies, post comment.
3. **Diff** ✅ — file fetch, patch parsing, virtualized `DiffRow` rendering,
   syntax highlighting.
4. **Review** ✅ — inline comments with pending-review batching, submit review,
   merge/close with confirmation, checks display.
5. **Polish** — keyboard navigation, filtering/search, notifications, offline
   cache, character-level text selection.

Each phase leaves a usable application.

## Status

Phases 1–4 are complete and verified against the live API. 279 tests pass;
clippy is clean across the workspace.

End-to-end verification (`cargo run -p rostrum --example review`) against real
pull requests confirms the parser's added/removed line counts match GitHub's own
reported totals exactly, and that every commentable line's anchor resolves to the
side GitHub expects.

Deliberately not built:

- **SQLite cache and ETag storage.** The store is memory-only, so each launch
  starts cold. Everything else assumes a cache exists; adding one is additive.
- **Character-level text selection in rendered prose.** GPUI has no read-only
  selection primitive, and hand-rolling one is a few hundred lines (see
  `docs/features/ui_foundation.md`). Links are clickable; code and comment text
  are not yet selectable.
- **Multi-line inline comments.** `DraftComment` carries `start_line`/
  `start_side` and the API layer sends them, but the UI only creates
  single-line anchors.
- **`head_sha` tagging of pending reviews.** The design calls for pending
  comments to be tagged with the commit they were written against so a
  force-push can invalidate them; the PR query does not currently fetch
  `headRefOid`, so this check is absent.
- Keyboard navigation, filtering, and notifications (phase 5).
