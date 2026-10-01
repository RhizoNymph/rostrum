# Rostrum — Overview

```yaml
Overview:
  description: >
    A native Rust desktop application built on GPUI that aggregates open pull
    requests — and, on a second tab, open issues — across a user-configured
    set of GitHub repositories into a single vertically scrolling feed, and
    provides full in-app review: reading the PR body and comment chain, viewing
    the diff with syntax highlighting, leaving inline line comments, submitting
    reviews, and merging; and for issues, reading, commenting, closing,
    reopening, labelling, assigning and creating them.

  subsystems:
    ui_foundation: >
      Theme, typography, and the local component layer built directly on `gpui`.
      Owns text rendering, the hand-rolled selection/copy primitive, and the
      markdown renderer. No dependency on Zed's `ui`/`theme` crates (GPL).
    repo_feed: >
      The primary screen. Flattens all repos and their PRs — or, on the Issues
      tab, their issues — into a single row stream rendered by one virtualized
      `list`, styled to look like discrete per-repo containers.
    repo_view: >
      One repository on its own screen (desktop). The left pane becomes that
      repository's pull requests and issues, split top and bottom, both in the
      feed's item sort; with nothing selected the right pane shows the branch
      tree — trunks against the default branch, pull requests against their
      base, stacks nested.
    issues: >
      The Issues tab and everything behind it: fetching and caching each
      repository's open issues in the poll cycle, the issue pane (header with
      editable labels and assignees, timeline, composer, close/reopen), and the
      new-issue form. Model, decoding, requests, cache and form rules are
      gpui-free.
    pr_detail: >
      Master/detail right pane. Tabbed Conversation / Files / Checks view for a
      selected PR, including the comment composer and the PR-level actions:
      comment, review, merge, close, and draft conversion.
    diff_review: >
      Unified-diff parsing, syntax highlighting, virtualized diff rendering,
      inline comment threads, and pending-review batching.
    github_sync: >
      All network I/O. Token acquisition, GraphQL reads, REST mutations, the
      polling scheduler, the SQLite cache, and rate-limit handling.
    local_git: >
      The local half of a pull request: which worktree a branch is checked out
      in, how far it has drifted from GitHub and from its base, and pull/merge/
      rebase on it — one at a time from the detail pane, or across every open
      pull request from the feed. Drives the `git` command line; never writes
      to a remote, except the one leased push behind arranging a stack.
    feed_sort: >
      Ordering the feed: repository containers by pushed, updated, created,
      owner, name or stars, and the items in each by pushed, updated,
      created, author or title, each either way. Pure comparisons in
      rostrum-core (with a hook for groups that sort as one unit), applied by
      `flatten`, persisted with the other feed preferences.
    stacks: >
      Stacks of pull requests. Reads GitHub's stacks (Stacks REST API, cached),
      detects chains that could be one, renders each stack under a header in
      its repository's container and sorts it as one unit; makes stacks from
      a clone (`gh stack link`/`init`, rebasing and lease-pushing branches
      first when arranging arbitrary pull requests), adds pull requests to the
      top of an existing stack (`gh stack link <stack> <pr>...`, rebasing onto
      the top first when they do not already chain), merges a whole stack
      atomically (`gh stack merge`) and unstacks. A paired phone drives the
      same operations through rostrumd; the phone's own UI is not built yet.
    author_filter: >
      Narrowing the feed to chosen people — authored, or optionally also
      assigned/review-requested — and the persistence of every feed setting
      that is a standing preference rather than a half-finished search.
    remote_protocol: >
      The contract between a paired phone and `rostrumd`: pairing codes and
      links, device tokens, certificate pinning, and the authenticated API for
      local state, local jobs, sync-all and handoff sessions.
    android_core: >
      The phone app's Rust half. Wraps the gpui-free crates behind one UniFFI
      object, so Kotlin renders snapshots and rows the core has already
      filtered, highlighted and anchored, and never stores a secret in Rust.
    conflict_handoff: >
      When a local rebase or merge stops on conflicts and a handler is
      configured, leaves the worktree in place and spawns the handler in a
      named tmux session with a pre-gathered context bundle.
    android_build: >
      The Android app's toolchain. A Gradle project under `android/`
      cross-compiles `rostrum-ffi` for each Android ABI with cargo-ndk,
      generates its Kotlin bindings with UniFFI, packages both into a signed
      APK, and publishes that APK for `rostrumd` to serve to phones. It also
      owns the Compose theme foundation (palette, Material mapping, fonts).
    android_app: >
      The Android client. Jetpack Compose screens for the feed, a pull
      request (conversation, files, checks, branch), the single-file diff with
      inline comments and pending reviews, merging, settings, the paired
      desktop (including copying its settings onto the phone), sign-in and
      pairing. ViewModels depend on one Kotlin interface,
      `RostrumBackend`, shaped after `RostrumCore`; secrets are sealed with an
      Android Keystore key; WorkManager runs the notification check.
    android_issues_repo: >
      The phone's feed sort and Pull requests | Issues tabs, the issue screen
      (with editing) and new-issue form, stacks drawn together in the feed and
      their actions run on the paired desktop as polled jobs, "load earlier"
      on conversations, and a repository's own screen with its branch tree and
      trunks — ordered and assembled by the core, drawn by the app.
    android_profiles: >
      Several paired desktops on one phone, each a profile with its own
      repositories, filters, cache, drafts and GitHub account (a desktop's
      handover or a pasted token). The core's profile registry holds one core
      per profile; the app keeps each profile's secrets under its id, shows the
      active profile only, switches between them, and checks every profile
      for notifications.
    rostrumd: >
      A headless desktop daemon, run as a systemd user service. Its plain-HTTP
      page (LAN and tailnet) offers the Android APK and — from this computer or
      over the tailnet only — generates pairing codes and revokes phones; its
      HTTPS API (self-signed, pinned by fingerprint) serves the
      remote_protocol routes by driving rostrum-local on the configured
      clones, and drives stacks (make, arrange, add to stack, merge, unstack)
      through rostrum-stack as polled jobs. State is owned by two actors:
      pairing codes and devices, and which clone is busy (plus the jobs).

  data_flow: >
    At startup the app resolves a GitHub token (`gh auth token`, falling back to
    $GITHUB_TOKEN) and loads config + cached state from SQLite, so the feed
    paints before any network round-trip completes. `Config::feed_filter()`
    seeds `AppState.filter` in the same step, so the feed's standing
    preferences — hidden drafts, hidden empty repos, the author selection —
    are in force on the first painted frame rather than snapping on later.

    `SyncEngine` (a GPUI entity) then runs a poll loop: one GraphQL query per
    configured repo, staggered, guarded against overlap by an in-flight `Task`
    handle. Network futures execute on a Tokio runtime bridged into GPUI's
    executor; results are applied back on the main thread via
    `entity.update(cx, ..)` followed by `cx.notify()`.

    `AppState` holds the canonical `Vec<RepoState>`. Whenever it changes, the
    feed's flat `Vec<FeedRow>` is rebuilt and pushed into `ListState` via
    `splice`, which is what actually drives re-render of the scrolling feed.
    `flatten` lays repositories and their items out in the order
    `FeedFilter::sort` names; the feed query carries the repository facts
    (push and creation times, stars, owner) those orders read, and they are
    cached beside the pull requests so a sorted feed opens in order.

    Each refresh also fetches the repository's open issues — a second GraphQL
    document with its own overlap guard and its own `LoadState` on
    `RepoState` — and caches them, so a cold start paints both tabs.
    `AppState.tab` (persisted as `feed_tab`) picks which list `flatten_tab`
    builds; `Selection` is an enum over a pull request and an issue, by
    `(RepoId, number)`, and the workspace's `DetailPane` follows it: a
    `PrDetail`, an `IssuePane`, or — not a selection — the new-issue form.
    Issue mutations go out over REST as `IssueMutation`s and reload the pane
    and the repository's issues authoritatively.

    Selecting a PR creates a `PrDetail` entity, which lazily fetches the
    conversation timeline and, on first visit to the Files tab, the changed-file
    patches. Patches are parsed into `DiffRow`s carrying old/new line numbers;
    those line numbers are what inline comments are anchored to when submitted.

    Conversations (pull request and issue) are fetched newest page first; each
    long connection carries a cursor and total in `Conversation.paging`, "Load
    earlier" merges the previous page through `rostrum-core`'s pure
    `merge_earlier`, a reload keeps loaded pages via `refreshed_by`, and the
    merged set is what gets cached. Editing an issue's title and description
    re-reads the issue before the REST PATCH and stops at a conflict when
    someone else changed either since the editor opened.

    Opening a repository's view (`o`, or its header's name) switches the
    workspace's `Screen` to that repository without touching the feed entity,
    so the feed's scroll survives the round trip. A `RepoBranches` entity
    fetches the repository's default branch, stars and trunk refs, resolves
    the trunks from `config.json`, then sends every trunk-vs-default and
    pull-request-vs-base comparison as one aliased `Ref.compare` batch; it
    fetches again whenever the feed's poll lands a refresh for the
    repository. `build_tree` (pure, in `rostrum-core`) turns the trunks, the
    current pull requests and the counts into the branch tree on every paint.

    Filter changes run the other way: every persisted toggle goes through
    `Store::edit_filter`, which applies the edit, folds it back into `Config`
    via `absorb_filter`, and writes the file. `feed_filter`/`absorb_filter` are
    inverses and the only reader/writer of those fields, so what is saved and
    what is restored cannot drift apart. The search query is the one filter
    excluded, deliberately. The two feed sorts ride the same funnel, but are
    not filters: they never count as an active filter and survive "clear".

    Stacks ride beside the pull requests. After each refresh the store reads
    the repository's stacks from GitHub's Stacks API into `RepoState::stacks`
    (and `cache_stack`), and `flatten` groups each repository's visible pull
    requests into units — GitHub's stacks, then chains detected from base and
    head branches — sorting a stack as one unit through the sort's group
    hook and emitting a `StackHeader` row before its members. Only the Pull
    requests tab groups; issues are never in a stack. A repository's own view
    lays its pull requests out the same way (`repo_pull_rows`), and its branch
    tree marks stack members. Stack actions
    flow out through `rostrum-stack`: one operation at a time, run on Tokio,
    driving `git` in the clone (scratch worktrees, the leased push) and `gh
    stack` through its one runner, with progress returned over a channel and
    a refresh when it ends.

    Mutations (comment, review, merge) go out over REST, are applied optimistically
    to local state where safe, and are reconciled by the next poll. Draft
    conversion and updating a branch from its base are the exceptions: REST
    cannot express either one fully, so both go out as GraphQL mutations keyed by
    the pull request's node id.

    On Android the same gpui-free crates run behind `rostrum-ffi`. A
    `ProfileRegistry` keeps one profile per paired desktop (or GitHub token),
    each its own data directory and its own `RostrumCore`; the feed shows the
    active profile and the notification job walks them all. The phone lays
    its feed out with `FeedOrder::AsListed` — repositories in the user's own
    settings order — until it grows a sort control. Compose calls
    suspend functions on the active profile's `RostrumCore`, whose state
    lives in an actor task; network I/O happens outside the actor and results
    are applied back through it, SQLite writes are queued in state order on a
    writer task, and feed changes reach Kotlin through a `FeedObserver` in
    revision order. Tokens arrive from Kotlin (the Keystore) and never touch
    disk on the Rust side. The paired desktop is reached through
    `rostrum-remote`'s pinned client.

    On Android, the Kotlin app reaches the same Rust crates through
    `rostrum-ffi`: Kotlin calls the UniFFI-generated bindings, which call into
    `librostrum_ffi.so` through JNA. The Gradle build produces the library and
    the bindings together, from one pinned uniffi version.

    Inside the app, every screen's ViewModel talks to the active profile's
    `RostrumBackend` and gets back `Outcome` values (never exceptions). Each
    profile (one per paired desktop, or per pasted token) has its own core
    from the core's profile registry, and `ProfileManager` owns them. At
    start-up it wipes the single-profile state of older builds, then each
    profile's `SessionRepository` unseals that profile's GitHub token and
    desktop pairing from app-private files and hands them to its backend,
    which keeps them in memory only; sign-in and pairing results flow the
    other way and are sealed again under the profile's id. Switching
    profiles rebuilds the navigation graph over the new profile's backend. The feed arrives as `FeedSnapshot`s, from calls and from the
    backend's update flow, newest revision winning. `rostrum://pair` links and
    notification taps enter through `MainActivity` into a link inbox; the
    root switches to a notification's profile first, then the navigation
    host drains it. The backend is `FfiRostrumBackend`: one `RostrumCore` per
    profile, each method one core call with its records
    and `RostrumException`s mapped to the app's model; the core's feed
    observer feeds the update flow. `FakeRostrumBackend` serves unit tests
    and previews only.
    The phone reaches the desktop's clones through rostrumd. Its page issues a
    one-time code as a `rostrum://pair` link and QR code carrying the LAN and
    tailnet addresses and the certificate fingerprint; the phone pins that
    fingerprint, exchanges the code for a device token (and the desktop's
    GitHub token), and presents the token on every API call. Each local call
    re-reads rostrum's `config.json` for the clone, the conflict handler and
    autostash, takes that clone's lease from the job coordinator (409 when it
    is busy), and runs the same `rostrum_local` function the desktop's button
    runs, in a task that outlives the request. Nothing is ever pushed. A
    paired phone can also copy the desktop's watched repositories and feed
    preferences (`GET /api/v1/config`) — never its clones, conflict handler,
    refresh interval or notifications.

    A phone drives stacks the same way it drives local jobs, but as jobs it
    polls: a stack request names the repository and pull requests; rostrumd
    takes the clone's lease, fetches the repository's open pull requests and
    stacks from GitHub, validates with rostrum-core's `plan_stack` /
    `plan_extend` (and, for a rewrite, that `confirm_rewrite` names exactly
    the branches `rewrites()` returns), then runs `run_stack_job`,
    `run_extend_job`, `merge_stack` or `unstack` with the configured clone,
    conflict handler and the desktop's scratch-worktree directory, recording
    progress and the outcome for `GET /api/v1/stacks/jobs/{id}`.

Features Index:
  ui_foundation:
    description: Theme, components, text rendering, selection, markdown.
    entry_points: [crates/rostrum-ui/src/lib.rs, crates/rostrum-md/src/lib.rs]
    depends_on: []
    doc: docs/features/ui_foundation.md
  repo_feed:
    description: Flattened, virtualized multi-repo PR feed.
    entry_points: [crates/rostrum/src/feed/mod.rs, crates/rostrum-core/src/feed.rs, crates/rostrum-core/src/tabs.rs]
    depends_on: [ui_foundation, github_sync]
    doc: docs/features/repo_feed.md
  repo_view:
    description: Per-repository view — pull requests and issues split in the sidebar (sorted, real issue rows and pane), the branch divergence tree, configurable trunks.
    entry_points: [crates/rostrum/src/repo_view/mod.rs, crates/rostrum-core/src/branches/mod.rs, crates/rostrum-core/src/navigation.rs]
    depends_on: [repo_feed, issues, feed_sort, pr_detail, github_sync, ui_foundation]
    doc: docs/features/repo_view.md
  issues:
    description: Issues tab, issue fetch and cache, issue pane with labels/assignees/close/reopen, and issue creation.
    entry_points: [crates/rostrum/src/issue/mod.rs, crates/rostrum/src/sync/issues.rs, crates/rostrum-core/src/issue.rs, crates/rostrum-core/src/tabs.rs, crates/rostrum-github/src/issues/mod.rs]
    depends_on: [repo_feed, pr_detail, github_sync, author_filter, ui_foundation]
    doc: docs/features/issues.md
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
  diff_overview:
    description: Visual overview of a diff — change map by directory/file churn, ranked largest-changes list, click-to-jump.
    entry_points: [crates/rostrum/src/detail/overview.rs, crates/rostrum-diff/src/overview.rs]
    depends_on: [ui_foundation, diff_review]
    doc: docs/features/diff_overview.md
  github_sync:
    description: Auth, GraphQL/REST client, polling, cache, rate limits.
    entry_points: [crates/rostrum-github/src/lib.rs, crates/rostrum/src/sync/mod.rs]
    depends_on: []
    doc: docs/features/github_sync.md
  local_git:
    description: Worktree-aware clone status, divergence, pull/merge/rebase, sync-all.
    entry_points: [crates/rostrum-git/src/lib.rs, crates/rostrum-local/src/lib.rs, crates/rostrum/src/sync/mod.rs]
    depends_on: [pr_detail, repo_feed]
    doc: docs/features/local_git.md
  feed_sort:
    description: Repository and item sorts — keys, directions, group aggregation, the Sort popover, persistence.
    entry_points: [crates/rostrum-core/src/sort/mod.rs, crates/rostrum-core/src/sort/compare.rs, crates/rostrum/src/feed/sort_menu.rs]
    depends_on: [repo_feed, github_sync, author_filter]
    doc: docs/features/feed_sort.md
  stacks:
    description: >
      Stacks of pull requests — GitHub's (Stacks API, cached) and detected
      chains — grouped and sorted as one unit in the feed; Make stack, Arrange
      (rebase + leased force-push), Add to stack (append to an existing
      stack's top), atomic Merge stack, and Unstack via `gh stack`.
    entry_points: [crates/rostrum-core/src/stack/mod.rs, crates/rostrum-stack/src/lib.rs, crates/rostrum/src/feed/stacks.rs, crates/rostrum/src/sync/stacks.rs]
    depends_on: [repo_feed, feed_sort, github_sync, local_git, conflict_handoff]
    doc: docs/features/stacks.md
  author_filter:
    description: Author/involvement filtering of the feed, and persisted feed settings.
    entry_points: [crates/rostrum-core/src/authors.rs, crates/rostrum-config/src/lib.rs]
    depends_on: [repo_feed, github_sync]
    doc: docs/features/author_filter.md
  conflict_handoff:
    description: Hand a stopped rebase/merge to a configured command in tmux, with context.
    entry_points: [crates/rostrum-handoff/src/lib.rs, crates/rostrum-git/src/context.rs]
    depends_on: [local_git]
    doc: docs/features/conflict_handoff.md
  remote_protocol:
    description: Phone ↔ desktop protocol — pairing, device tokens, pinned TLS client, API types.
    entry_points: [crates/rostrum-remote/src/lib.rs, crates/rostrum-remote/src/client.rs]
    depends_on: [local_git]
    doc: docs/features/remote_protocol.md
  android_core:
    description: >
      The Android app's Rust core behind UniFFI — a ProfileRegistry with one
      profile per paired desktop or GitHub token, each a RostrumCore serving
      the feed (pull request and issue tabs, the saved sorts, stacks as
      units), stack actions through the paired desktop, pull request and
      issue detail with every issue action, issue creation and editing, and
      "load earlier" paging, a repository's own screen with its branch
      tree and trunks, diff rows, pending review, desktop pairing and jobs,
      copying the desktop's config, and background notifications, all
      render-ready for Compose.
    entry_points: [crates/rostrum-ffi/src/lib.rs, crates/rostrum-ffi/src/profiles/mod.rs, crates/rostrum-ffi/src/engine/mod.rs, crates/rostrum-ffi/src/issues/mod.rs, crates/rostrum-ffi/src/repo_view/mod.rs, crates/rostrum-ffi/src/stack_actions/mod.rs]
    depends_on: [repo_feed, pr_detail, diff_review, diff_overview, author_filter, github_sync, remote_protocol, feed_sort, issues, stacks, repo_view]
    doc: docs/features/android_core.md
  android_build:
    description: Gradle project, cargo-ndk + UniFFI pipeline, signing, and APK publishing for the Android app.
    entry_points: [android/scripts/build-apk.sh, android/app/build.gradle.kts, crates/rostrum-ffi/src/lib.rs]
    depends_on: []
    doc: docs/features/android_build.md
  android_app:
    description: Compose UI, RostrumBackend over the Rust core (FfiRostrumBackend), Keystore secrets, session, deep links, notifications.
    entry_points: [android/app/src/main/kotlin/io/github/rhizonymph/rostrum/RostrumApplication.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/data/RostrumBackend.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/data/ffi/FfiRostrumBackend.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/navigation/RostrumNavHost.kt]
    depends_on: [android_build, android_core, android_profiles]
    doc: docs/features/android_app.md
  android_issues_repo:
    description: Feed sort sheet and Pull requests | Issues tabs, issue screen with editing and new-issue form, stacks in the feed with their actions on the paired desktop, load earlier, repository screen with branch tree and trunk editor, on Android.
    entry_points: [android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/feed/FeedSortSheet.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/issue/IssueRoute.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/newissue/NewIssueRoute.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/repo/RepoRoute.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/items/ItemRows.kt]
    depends_on: [android_app, android_core, feed_sort, issues, stacks, repo_view]
    doc: docs/features/android_issues_repo.md
  android_profiles:
    description: One profile per paired desktop or pasted token; ProfileManager over the core's profile registry, per-profile secrets, switching, pairing into profiles, notifications across profiles.
    entry_points: [android/app/src/main/kotlin/io/github/rhizonymph/rostrum/data/profiles/ProfileManager.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/data/ffi/FfiProfileRegistry.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/app/RostrumApp.kt, android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/profiles/ProfileSwitcherSheet.kt]
    depends_on: [android_core, android_app]
    doc: docs/features/android_profiles.md
  rostrumd:
    description: Desktop daemon — pairing page and APK download (HTTP), the phone's local-git and stack API (HTTPS), systemd user service.
    entry_points: [crates/rostrumd/src/main.rs, crates/rostrumd/src/app.rs, crates/rostrumd/src/api/mod.rs, crates/rostrumd/src/web/mod.rs, crates/rostrumd/src/stacks/mod.rs]
    depends_on: [remote_protocol, local_git, conflict_handoff, github_sync, stacks]
    doc: docs/features/rostrumd.md
```

## Workspace layout

Non-UI logic lives in crates that do not depend on `gpui`, so the bug-prone parts
(diff line mapping, GraphQL decoding, feed flattening) are testable with plain
`cargo test` and no window.

| Crate | gpui? | Responsibility |
|---|---|---|
| `rostrum-core` | no | Domain types (pull requests and issues), feed flattening per tab and sorting, conversation model, branch tree, screen navigation |
| `rostrum-db` | no | SQLite cache (pull requests, issues, repository metadata, conversations) and draft persistence |
| `rostrum-github` | no | GraphQL reads, REST mutations, auth, rate limiting, errors |
| `rostrum-diff` | no | Unified-diff parsing, `DiffRow` model, syntax highlighting |
| `rostrum-git` | no | Worktrees, clone status, divergence, pull/merge/rebase, conflict context via the `git` CLI |
| `rostrum-handoff` | no | Context bundle rendering and tmux session spawning for conflict handoff |
| `rostrum-local` | no | One pull request's local state (`local_state`) and one local operation on it (`run_local_job`), shared by every caller |
| `rostrum-stack` | no | Stacks that act: the `gh stack` runner and its typed commands, reading gh-stack's local file, making/arranging (`run_stack_job`), merging and unstacking |
| `rostrum-remote` | no | Phone ↔ desktop protocol: pairing, device tokens, API types, and (feature `client`) the pinned HTTPS client |
| `rostrumd` | no | Desktop daemon: pairing page and APK download over HTTP, the paired phone's API over HTTPS, systemd user service |
| `rostrum-config` | no | `config.json`: watched repositories, clones, feed preferences, conflict handler |
| `rostrum-md` | no | `pulldown-cmark` → renderable markdown model |
| `rostrum-ffi` | no | The Android app's core: `RostrumCore` over UniFFI (`cdylib`), plus the `uniffi-bindgen` binary |
| `rostrum-ui` | yes | Theme, components, text/selection, markdown element |
| `rostrum` | yes | Bootstrap, window, root views, `SyncEngine` |

## Foundational decisions

| Decision | Choice | Rationale |
|---|---|---|
| Auth | `gh auth token`, `$GITHUB_TOKEN` fallback | No secret storage of our own; `gh` handles SSO and refresh |
| Reads | GraphQL v4 | One round-trip per repo instead of dozens; cost-based rate limit |
| Issues | A second GraphQL document per repository, not a selection on the pull request query | Separate load states per list, no coupling to the merge-probe timer; one extra point-1 request per repository per poll |
| Mutations | REST v3, except draft conversion and branch updates | Simpler, better-documented endpoints for merge/review/comment. REST accepts `draft` only at creation, and its `update-branch` endpoint can only merge, so those two go through GraphQL |
| Node ids | Fetched with the feed query | GraphQL mutations address a pull request by node id only. Carrying it on `PullRequest` makes a conversion one round trip, and is what the other GraphQL-only operations will need |
| UI deps | `gpui` + `gpui_platform` only | Zed's `ui`/`theme`/`syntax_theme` are GPL-3.0-or-later |
| Diff parsing | hand-rolled | `diffy` requires `---`/`+++` headers GitHub's per-file patches lack, and exposes neither `\ No newline` nor the raw `@@` line |
| Highlighting | `syntect` (pure-Rust regex) | One dependency covering many languages, versus matching the tree-sitter ABI across a grammar crate per language. Tree-sitter remains the better long-term choice |
| Local git | Drive the `git` CLI, not libgit2 | Inherits the user's credential helpers, ssh agent, hooks, and `rerere` for free; libgit2's rebase is a partial substitute and its credential negotiation would have to be reimplemented. `auth.rs` already shells out to `gh` |
| Local writes | Never push — with one exception: rebasing pull requests into a stack (Arrange, or Add to stack when the additions do not already chain) | A local merge or rebase leaves the clone ahead, and that count is the cue to push. Arranging is the exception because rebasing a branch onto another is invisible to its pull request until pushed. It goes through one function (`Repo::push_with_lease`, always `--force-with-lease=<ref>:<expected-oid>`, never a bare force), runs only after every member rebased cleanly, never on a stack's existing members, only behind an explicit confirmation naming the branches, and `gh stack` is never allowed to push on rostrum's behalf. See `docs/features/stacks.md` |
| Stacks | GitHub's Stacks API is the source of truth; `gh stack` performs every stack write | The API needs no clone, so every watched repository groups; gh-stack owns link/merge/unstack semantics, and its local file is read, never written |
| `gh` environment | Inherited (like tmux), minus `GIT_DIR`-family variables, with `GH_REPO` pinned | `gh` authenticates with the user's own setup; the removed variables would change which repository a nested `git` reads, and a pinned `GH_REPO` stops a fork remote redirecting a merge |
| Feed distance | One batched `Ref.compare` per repository after each refresh | A GraphQL field cannot read a sibling's value, so the count cannot join the feed query; aliasing one `compare` per PR keeps it to one request, cost 1 |
| Conflicts | Abort by default; leave and hand off to tmux when a handler is configured | Rostrum has no conflict editor. Either the clone is left as found, or something that can edit is running in a named session with the context gathered |
| Phone access | `rostrumd`: an HTTPS API on a self-signed certificate the phone pins by fingerprint, and a plain-HTTP page whose pairing half answers only loopback and the tailnet | No CA and no certificate warning anywhere; the page opens in any browser; a pairing code — the key to the clones and the GitHub token — cannot be minted from the shared LAN |
| Handoff environment | tmux inherits rostrum's full env; `rostrum-git` uses an allowlist | The two spawn different things for different reasons: git's output is parsed and must be deterministic; the harness is the user's own tool and needs their `PATH`, `DISPLAY`, and keys |
| Cache | SQLite via `sqlx` | Instant cold start, offline reads, ETag storage |
| Async | Tokio bridged into GPUI's executor | GPUI's executor is not Tokio; `reqwest` requires a Tokio reactor |
| Branch tree | Trunks configured per repository (default: whichever of `main`/`master`/`staging`/`develop` exist), counts from one aliased `Ref.compare` batch, tree built in `rostrum-core` | A commit graph would need the history GitHub's API pages through slowly; counts answer "how far apart" in one request, and a pure builder makes nesting, loops and unknown bases testable |
| Mergeability | `mergeable` **and** `mergeStateStatus`, collapsed into one `MergeStatus` in `rostrum-core` | `mergeable` cannot distinguish "blocked by a required review" from "behind its base"; deriving the verdict once keeps the chip, the button, and its tooltip from disagreeing |

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

## Android build prerequisites

The Android app (`android/`, see `docs/features/android_build.md`) needs the
following on top of the Rust toolchain. `android/scripts/build-apk.sh` checks
each one and names the fix for any that is missing.

- Rust targets `aarch64-linux-android` and `x86_64-linux-android` for the
  active toolchain (`rustup target add ...`).
- `cargo-ndk` 4.1.2 (`cargo install cargo-ndk --version 4.1.2 --locked`).
- Android SDK with platform `android-36` and build-tools, located by `sdk.dir`
  in `android/local.properties` (written from `$ANDROID_HOME` or
  `~/Android/Sdk` if missing).
- Android NDK r30 (`30.0.16248370`, the `ndk` entry in
  `android/gradle/libs.versions.toml`) in `<sdk>/ndk/<version>`, or
  `$ANDROID_NDK_HOME`.
- A JDK 21 that Gradle can detect (the daemon and the compile toolchain), plus
  any Java to launch `gradlew`.
- For signed release builds, `android/.env` pointing at the keystore outside
  the repository (`~/.config/rostrum/android/release.jks`). Without it, release
  builds fall back to the debug key.

Use the rustup `cargo` (`~/.cargo/bin/cargo` or `$CARGO`), never the one on
PATH. On this machine that is a shim that may run builds on a remote host
without the NDK.

## Dependency sourcing

`gpui`, `gpui_platform`, and `gpui_tokio` are pinned to one Zed git rev in the
root `Cargo.toml`. The rev is spelled out three times rather than shared, because
cargo has no way to factor it out; moving it means editing all three lines
together, and the three must never disagree.

Zed's git history is ~500 MB, so the first build clones it into cargo's shared
git cache (`~/.cargo/git`). That cost is paid once per machine per rev, not once
per project. Iterating on GPUI itself is the case that wants a local checkout:
swap all three for `path` deps into its `crates/`, and swap them back before
committing.

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
5. **Polish** ✅ — keyboard navigation, filtering, notifications, offline cache,
   text selection.

Each phase leaves a usable application.

## Status

All five phases are complete and verified against the live API. 1497 tests pass
(154 of them in `rostrum-ffi`); clippy is clean across the workspace.

Issues are on the desktop: a tab beside pull requests, an issue pane with
title and description editing, and issue creation; see `docs/features/issues.md`. The phone does not show them
yet.

The Android app's core, `rostrum-ffi`, exposes the same feed, detail, diff,
review, desktop and notification behaviour to Kotlin through UniFFI, one
profile per paired desktop (or GitHub token), each with its own data; see
`docs/features/android_core.md`.

End-to-end verification (`cargo run -p rostrum --example review`) against real
pull requests confirms the parser's added/removed line counts match GitHub's own
reported totals exactly, and that every commentable line's anchor resolves to the
side GitHub expects.

Deliberately not built:

- **Cross-block text selection.** Selection works within a rendered markdown
  block and, in the diff, over whole lines. Dragging from one paragraph into the
  next does not extend the selection.
- **Squash and rebase merges on the desktop.** `MergeMethod` models all three
  and the API layer sends whichever it is given, but the desktop UI only offers
  a plain merge. The Android core offers all three.
- **Resolving review threads.** Threads render with their resolved state; there
  is no button to resolve one.
- **Editing or deleting your own comments.** (An issue's own title and
  description can be edited.)
- **Paging inside a review thread.** A thread's comments are capped at its
  first 50; the threads themselves, and every other long connection of a pull
  request or issue, are paged.
- **Paging on the phone.** The Android core caches and decodes the paged
  conversation, but offers no "load earlier" yet.
