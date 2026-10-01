# Feature: android_core

`rostrum-ffi`: the Rust core of rostrum's Android app, exposed to Kotlin
through UniFFI 0.32.1. The Jetpack Compose UI calls this crate and nothing
else. The crate wraps the same gpui-free crates the desktop uses, so the two
apps agree on every rule — filtering, merge verdicts, comment anchoring,
stale drafts, notification arrivals — because there is one implementation of
each.

## Scope

- Profiles: a `ProfileRegistry` holding one profile per paired desktop (or
  per bare GitHub token), each with its own `RostrumCore` — its own
  repositories, feed filters, cache, drafts, remote and GitHub account — and
  which profile is active.
- One UniFFI object per profile, `RostrumCore`, opened over that profile's
  data directory, owning the settings file, the SQLite cache and drafts, the
  GitHub session and the paired desktop.
- The GitHub session: a token handed in by Kotlin (pasted, or from the
  desktop), its verification, and the viewer it belongs to.
- Settings: watched repositories (typed validation), refresh cadence,
  pull requests per repository, notification toggles, the desktop-job stash
  default, and the feed's persisted filter preferences.
- The feed: cached and refreshed snapshots, per-repository load state and
  body, filtering, collapse, the author roster, distance from base, and
  background merge-state re-checks delivered through an observer. Two tabs
  (pull requests, issues) with per-tab counts, and the saved repository and
  item sorts (`docs/features/feed_sort.md`).
- Stacks (`docs/features/stacks.md`): the feed and the repository screen
  deliver a stack as one item — header, then its visible members bottom
  first — sorting as one unit, as on the desktop. Making, arranging,
  extending, merging and unstacking run on the paired desktop as jobs the
  phone starts and polls; eligibility and plan checks answer locally from
  the cached feed.
- Issues (`docs/features/issues.md`): the Issues tab's rows, the issue
  screen (header plus timeline, markdown flattened), comment, close as
  completed or not planned, reopen, labels, assignees, and creating an
  issue. Issues are cached like pull requests.
- Editing an issue's title and description, with the desktop's conflict
  check (`IssueEditor`) and an explicit overwrite; "load earlier" on issue
  and pull request conversations, the merged pages cached with their
  cursors.
- A repository's own screen (`docs/features/repo_view.md`): its pull
  requests (stacks grouped) and issues unfiltered in the item sort, its
  facts, its branch tree with ahead/behind, and its trunk setting.
- Pull request detail: header with the merge verdict and draft action,
  the conversation with markdown flattened to blocks, threads, checks,
  labels, and every PR-level mutation (comment, reply, labels, merge with all
  three methods guarded by the expected head, close/reopen, draft toggle,
  update branch by merge or rebase guarded by the expected head).
- The Files tab: overview (stats, change map, ranked files, file list) and
  one file's diff as render-ready rows — syntax colours, word-level emphasis,
  comment anchors, inline threads and drafts.
- The pending review: drafts anchored by path/line/side (and start line),
  tagged with the head commit, persisted in SQLite before a call returns,
  stale when the head moves; submission as comment/approve/request-changes.
- The paired desktop: pairing by link or by address + probed fingerprint,
  the local worktree status and the four local jobs plus abort, sync-all,
  handoff sessions, fetching a fresh GitHub token, unpairing, and copying the
  desktop's repositories and feed preferences onto the phone.
- Background notifications: new pull requests and new review requests since
  the last check, against a persisted seen set.
- Structured logs forwarded to Kotlin.

## Non-scope

- Rendering, navigation, confirmation dialogs, secure storage. Kotlin owns
  these; the core says what needs confirming (merge, close) and never stores
  a secret.
- OAuth device flow. Tokens come from pairing or pasting.
- Scheduling. Kotlin's WorkManager calls `checkNotifications`; foreground
  polling is Kotlin's timer calling `refreshFeed` every
  `settings.refreshIntervalSecs`.
- Serving the desktop API (`rostrumd`) and running git — the desktop.
- Cross-compiling. The Android pipeline builds `-p rostrum-ffi` with
  cargo-ndk; this crate only guarantees it can: no C beyond bundled SQLite,
  `ring` not `aws-lc-rs`, no subprocesses.
- Resolving threads, editing or deleting comments, pagination past 100 —
  the same gaps as the desktop (see `docs/OVERVIEW.md`).
- Issues in background notifications.

## API

Kotlin names are camelCase; every call that touches state or I/O is
`suspend`. The package is `uniffi.rostrum_ffi`.

| Area | Methods |
|---|---|
| profiles | `ProfileRegistry.open(rootDir)` (not suspend), `profiles()`, `activeProfile()`, `setActiveProfile(id)`, `renameProfile(id, label)`, `setProfileLogin(id, login?)` (not suspend); `core(id) → RostrumCore`, `createTokenProfile(label)`, `pairDesktopWithLink(uri, deviceName) → ProfilePairing`, `pairDesktopManual(host, port, fingerprint, code, deviceName) → ProfilePairing`, `removeProfile(id)`; `parsePairingLink(uri)` (not suspend) and `probeDesktop(host, port)` for the Pair screen before any profile exists — the same code as the core's |
| lifecycle | `RostrumCore.open(dataDir)`, `warnings()` |
| session | `setGithubToken(token?) → GitHubStatus`, `githubStatus()`, `viewer() → UserRef` |
| settings | `settings()`, `addRepo(input) → "owner/name"`, `removeRepo(repo) → Boolean`, `setRefreshInterval(s)`, `setPrsPerRepo(n)`, `setNotifications(newPullRequests, reviewRequests)`, `setAutostash(b)` — setters return `Settings` |
| feed | `cachedFeed()`, `refreshFeed()`, `refreshRepo(repo)`, `setQuery(q)`, `setFilter(FeedPreferences)`, `toggleAuthor(login)`, `clearFilter()` (keeps the sort), `toggleCollapsed(repo)`, `setFeedTab(FeedTab)` — all return `FeedSnapshot`; `authorRoster(limit?) → AuthorRoster` (the active tab's authors), `setFeedObserver(FeedObserver?)` |
| sort | `sortSettings() → SortSettings`; `setRepoSort(RepoSortKey, SortDirection?)`, `setItemSort(ItemSortKey, SortDirection?)` → `FeedSnapshot` (no direction: a new key starts at its default, the same key keeps its direction) |
| issues | `issueDetail(repo, n) → IssueDetail`, `cachedIssueDetail(repo, n) → IssueDetail?`, `loadEarlierIssue(repo, n) → IssueDetail`, `editIssue(repo, n, title, body, baseUpdatedAt, overwrite) → IssueDetail` (throws `EditConflict(title, body, updatedAt)`), `commentOnIssue(repo, n, body)`, `closeIssue(repo, n, CloseIssueAs)`, `reopenIssue(repo, n)`, `addIssueLabel(repo, n, label)`, `removeIssueLabel(repo, n, label)`, `assignableUsers(repo) → List<UserRef>`, `addIssueAssignee(repo, n, login)`, `removeIssueAssignee(repo, n, login)`, `createIssue(repo, title, body, labels, assignees) → UInt` (the new number); labels to offer come from `repositoryLabels(repo)` |
| stack actions | `planStackRewrite(StackPlanRequest) → StackRewritePlan` (desktop dry run), `checkStackPlan(StackPlanRequest) → StackPlanCheck` and `stackCandidates(repo, stack) → List<StackCandidate>` (local, no network); `makeStack(repo, prs, trunk)`, `arrangeStack(repo, prs, trunk, confirmRewrite)`, `extendStack(repo, stack, prs, confirmRewrite)`, `mergeStack(repo, stack, StackMergeMethod)`, `unstack(repo, stack)`, `stackJob(id: ULong)` → `StackJob` |
| repository | `repoOverview(repo) → RepoOverview` (no network), `branchTree(repo) → BranchTree`, `trunks(repo) → TrunkSettings`, `setTrunks(repo, names?) → TrunkSettings` |
| detail | `pullDetail(repo, n)`, `cachedPullDetail(repo, n)`, `loadEarlierPull(repo, n) → PullDetail`, `pullHeader(repo, n)`, `repositoryLabels(repo)`, `addLabel`, `removeLabel`, `addComment`, `replyToThread(repo, n, threadId, body)`, `merge(repo, n, method, title?, message?, expectedHeadSha)`, `closePullRequest`, `reopenPullRequest`, `setDraft(repo, n, draft)`, `updateBranch(repo, n, method, expectedHeadOid)` |
| files | `filesOverview(repo, n) → FilesOverview`, `fileDiff(repo, n, fileIndex) → FileDiff` |
| review | `pendingReview`, `addDraft(repo, n, anchor, rangeStart?, body)`, `editDraft(…, draftId, body)`, `removeDraft(…, draftId)`, `discardDrafts` — all return `PendingReview`; `submitReview(repo, n, event, body, includeDrafts)` |
| remote | `parsePairingLink(uri)` (not suspend), `pairWithLink(uri, deviceName)`, `probeDesktop(host, port)`, `pairManual(host, port, fingerprint, code, deviceName)`, `setRemote(endpoint, deviceToken)`, `clearRemote()`, `remoteStatus()`, `machineInfo()`, `localStatus(repo, n)`, `runLocalJob(repo, n, op, autostash)`, `abortLocal(repo, n)`, `startSyncAll(op, autostash)`, `syncAllStatus()`, `handoffs()`, `refreshGithubTokenFromDesktop()`, `unpair()`, `desktopConfig() → DesktopConfigPreview`, `copyDesktopConfig() → Settings` |
| notifications | `checkNotifications() → List<NotificationEvent>`, `markNotificationsSeen()` |
| logging | top-level `installLogSink(LogSink, LogLevel)` |
| markdown | top-level `renderMarkdown(source, repo) → List<MdBlock>`: the composer's Preview, rendered exactly as the timeline will show it |

Key records and enums, by screen:

- **Feed**: `FeedSnapshot { revision, tab: FeedTab, tabCounts: TabCounts
  { pullRequests, issues }, sort: SortSettings, repos: [RepoSection],
  hiddenEmptyRepos, totalOpen, visibleOpen, query, preferences, filterActive,
  mergeStatesSettling, viewer }` — everything but `tabCounts` is for the
  active tab (`FeedTab` = `PULL_REQUESTS | ISSUES`). `RepoSection { repo,
  load: RepoLoad, openCount, visibleCount, collapsed, body: RepoBody }` where
  `RepoBody` is `Collapsed | Loading | Failed(reason) | Empty |
  Pulls(items: [PullItem]) | Issues(issues: [IssueSummary])` — straight from
  `rostrum_core::flatten_tab`, in the saved sort. `PullItem` is
  `Single(pull: PrSummary) | Stack(stack: StackSummary, members:
  [PrSummary])`. `PrSummary` carries number, title, author,
  created/updated `Instant`s, draft, CI state and colour role, review decision
  and chip, `MergeStatus` and chip, `BaseDivergence { behind, ahead, baseRef,
  fastForwards, summary }` and the `↓N` chip, labels with ARGB colours,
  +/−, `reviewRequested`, `isYours`, head/base refs.
  `AuthorChip.openItems` counts the active tab's items.
- **Sort**: `SortSettings { repoKey, repoDirection, repoDirectionLabel,
  itemKey, itemDirection, itemDirectionLabel, summary, repoOptions:
  [RepoSortOption], itemOptions: [ItemSortOption] }`; each option has `key,
  label, defaultDirection, descendingLabel, ascendingLabel` ("Newest first",
  "A→Z", "Most"…). `RepoSortKey` = `PUSHED | UPDATED | CREATED | OWNER |
  NAME | STARS`; `ItemSortKey` = `PUSHED | UPDATED | CREATED | AUTHOR |
  TITLE`; `SortDirection` = `ASCENDING | DESCENDING`.
- **Stacks**: `StackSummary { kind: StackKind, title, trunk, memberCount,
  absent, rollup: StackRollup? }`, `StackKind` = `GitHub(number) | Chain`,
  title `Stack 7 · 3 PRs` or `Stackable chain · 3 PRs`, `StackRollup {
  mergeable, total, worst: MergeStatus, label, role }`.
- **Stack actions**: `StackPlanRequest` = `Arrange(repo, prs, trunk) |
  Extend(repo, stack, prs)`; `StackRewritePlan { rewrites: [StackRewrite {
  number, branch }], needsRewrite }`; `StackPlanCheck` = `Valid(rewrites) |
  Invalid(reason)`; `StackCandidate { number, title, eligibility }` with
  `StackEligibility` = `Eligible(chained) | Ineligible(reason)`; `StackJob {
  id, repo, kind: StackJobKind, startedAt, finishedAt?, finished, state }`,
  `StackJobState` = `Running(progress?) | Done(result, detail) |
  Conflicted(number, detail) | HandedOff(number, session, worktree, detail)
  | Failed(pushed, detail)`, `StackJobResult` = `Stacked(rewritten,
  tracked) | Extended(stack, rewritten) | Merged(stack) |
  Unstacked(stack)`, `StackMergeMethod` = `MERGE | SQUASH | REBASE`.
- **Issues**: `IssueSummary { repo, number, title, url, status: IssueStatus,
  statusChip, author?, createdAt, updatedAt, labels, assignees,
  commentCount, milestone?, isYours, assignedToYou }`, `IssueStatus` =
  `Open | Closed(reason: IssueCloseReason?)`, `IssueCloseReason` =
  `COMPLETED | NOT_PLANNED | DUPLICATE`, `CloseIssueAs` = `COMPLETED |
  NOT_PLANNED`. `IssueDetail { issue, timeline: [TimelineEntry], hasEarlier,
  earlierCount? }`, the same entries as a pull request's.
- **Repository**: `RepoOverview { repo, url, stars?, defaultBranch?, pulls:
  [PullItem], issues: [IssueSummary], pullsLoad, issuesLoad }`. `BranchTree
  { repo, url, stars, defaultBranch?, trunks: TrunkSettings, rows:
  [BranchRow] }`, `BranchRow` = `Trunk(name, drift: TrunkDrift, pulls) |
  OtherBases | Base(name, pulls) | Pull(depth, number, head, base, drift:
  BranchDrift?, note: BranchNote?, stackLabel?, pull: PrSummary?)`,
  `TrunkDrift` = `Default | Missing | Unknown | Known(drift)`, `BranchDrift
  { ahead, behind }`, `BranchNote` = `BREAKS_CYCLE | AMBIGUOUS_BASE`,
  `TrunkSettings { detected, configured, existing }`.
- **Detail**: `PullDetail { header, timeline, threads, checks,
  unresolvedThreads, pendingReview, hasEarlier, earlierCount? }` — the last
  two default (`false`, `null`) on both detail records, so earlier Kotlin
  constructors still compile. `PullHeader` adds state
  (open/closed/merged), `headSha` (pass back as the expected head), reviewers,
  `MergeVerdict { status, sentence, blocksMerge, role, chip }`, divergence,
  and `DraftAction { toDraft, label }`. `TimelineEntry.kind` is
  `Description | Comment | Review | Event`, bodies as `List<MdBlock>`.
  `TimelineEvent` includes the issue events `ClosedAs(reason)`,
  `Unassigned(assignee)` and `CrossReferenced(source, title)`.
- **Markdown**: `MdBlock { kind, spans, quoteDepth, listDepth }` with
  `MdBlockKind` = `Paragraph | Heading(level) | Code(language?, code) |
  ListItem(ordered, number, checked?) | Rule | TableRow(cells, header) |
  Image(url, alt)` and `MdSpan { text, bold, italic, code, strike, link? }`.
  Nothing is recursive.
- **Files**: `FilesOverview { headSha, stats, changeMap: [MapColumn { share,
  tiles: [MapTile { fileIndex?, share, heat: TileHeat { removedRatio?, alpha
  } }] }], ranked: [RankedFile { additionsShare, deletionsShare }], files:
  [ChangedFile { index, availability, threads, drafts }] }`. `FileDiff { file,
  headSha, body: Rows(rows) | Unavailable }`; `DiffRow` = `Hunk | Line |
  Thread | Draft`; `DiffLineView { kind, oldLine?, newLine?, segments:
  [CodeSegment { text, color (ARGB), bold, italic, emphasized }], anchor:
  CommentAnchor?, noNewlineAtEof }`. Segments carry their own text, so Kotlin
  never converts UTF-8 offsets to UTF-16.
- **Review**: `PendingReview { drafts: [ReviewDraft { id, anchor, body,
  location }], draftedAgainst?, headSha, stale }`, `CommentAnchor { path,
  line, side }`, `ReviewEvent`.
- **Copying the desktop's config**: `DesktopConfigPreview { machine, repos,
  added, removed, prsPerRepo, hideDrafts, hideEmptyRepos, authors,
  includeInvolved, autostash, changesAnything }` — `repos` is the desktop's
  list in its order, `added` what the phone lacks, `removed` what copying
  drops.
- **Remote**: `PairingPreview`, `DesktopProbe`, `PairingResult { machine,
  endpoint, deviceId, deviceToken, github? }`, `RemoteStatus`, `MachineInfo`,
  `LocalStatus = NotConfigured | NotCheckedOut | CheckedOut(LocalBranch)`,
  `LocalOp` (4), `SyncAllOp` (3 — merging the remote into every worktree is
  deliberately not offered), `JobResult { outcome, detail, chip? }`,
  `SyncRun { entries, summary, progressText }`, `HandoffSession`.
- **Notifications**: `NotificationEvent { kind: NewPullRequest |
  ReviewRequested, repo, number, title, author?, url }`.

Errors are one sealed class, `RostrumException`: `NotSignedIn`,
`GitHubAuthFailed`, `GitHubRateLimited(resetsAt)`, `MergeBlocked(reason)`,
`GitHubApi(status?, reason)`, `Network`, `UnknownPullRequest`,
`DraftsStale(draftedAgainst, head)`, `EditConflict(title, body, updatedAt)`, `NotPaired`, `DeviceRevoked`,
`DesktopUnreachable`, `CertificateMismatch(host)`, `DesktopTimeout`,
`IncompatibleDesktop`, `RemoteApi(code, reason)`, `RewriteNotConfirmed(branches, reason)`, `RemoteProtocol`,
`InvalidRepo`, `DuplicateRepo`, `InvalidInput(reason)`, `Storage`, `Internal`.
`describe()` gives a sentence. No variant has a field named `message`: it
would collide with `Throwable.message` in the generated class.

Colours: chips carry a `ColorRole` (success/warning/danger/draft/accent/
neutral) for Kotlin to map onto its palette. The only literal colours are
GitHub label colours and syntax colours (syntect `base16-ocean.dark`, the
desktop's theme), both opaque ARGB `u32`.

## Data and control flow

```
Compose UI ──suspend call──▶ UniFFI scaffolding (async_runtime = "tokio")
                               │ the future is polled on the caller's thread
                               │ inside async-compat's Tokio context
                               ▼
                         RostrumCore method (exported impl in each area module)
          ┌────────────────────┼──────────────────────────────┬─────────────────┐
          ▼                    ▼                              ▼                 ▼
  state actor (mailbox)   network, outside the actor    blocking pool      SQLite reads
  CoreState: config,      GitHubClient  (reqwest+ring)  highlighting,      (rostrum-db,
  session, feed, known    RemoteClient  (pinned TLS)    markdown, diff     own pool)
  PRs, drafts, caches,         │                        rows, parsing
  probes, baseline             │ results sent back as closures
          │◀───────────────────┘
          ├──▶ writer task ──▶ rostrum-db: cache, drafts (acked), seen set
          ├──▶ config.json via rostrum-config (in the actor, write-then-apply)
          └──▶ notifier task ──▶ FeedObserver.feedChanged(snapshot), in order
```

### Concurrency

- **One owner for mutable state.** `engine::actor` runs a task that owns
  `CoreState` and executes closures sent over an unbounded channel, one at a
  time; `Actor::call` returns the closure's result over a oneshot. Closures
  never await, so no call holds the state across a network or disk
  operation. A panicking closure fails only its own call.
- **Ordered writes.** SQLite writes are queued from inside actor closures
  onto the single `engine::writer` task, so they land in the order the state
  changed. Draft and seen-set writes carry an acknowledgement the method
  awaits: `addDraft` returns only once the draft is on disk.
- **Ordered delivery.** Feed snapshots for the observer are queued from the
  actor and delivered by `engine::notifier`, one at a time, each on the
  blocking pool so a slow observer never stalls the runtime. Every snapshot
  has a `revision`; the observer sees them in increasing order, and a newly
  registered observer receives the current feed at once.
- **Background work dies with the core.** Merge-state probes hold a weak
  actor handle and upgrade it only to report; dropping `RostrumCore` closes
  the mailbox, ends the actor, drops the probe slots (which abort their
  tasks), and lets the writer drain and stop.
- **Runtime.** Everything runs on the Tokio context UniFFI's
  `async_runtime = "tokio"` provides (async-compat's runtime on device, the
  test runtime in tests). CPU work uses `spawn_blocking`. No threads are
  created by hand.

### Feed refresh

1. `ensure_hydrated` fills repositories that have nothing yet from SQLite,
   once (never over network data).
2. The actor issues a plan: the client, the per-repository limit, and a
   sequence number per repository (a full refresh also starts a new probe
   cycle). Repositories with nothing to show get a spinner.
3. Up to four repositories are fetched at once; each fetch is the feed query
   followed by the batched `Ref.compare` divergence query. A failed batch
   leaves the counts to carry forward.
4. Each result is applied as it lands, and only if no newer fetch of that
   repository landed first: `carry_forward_divergence`, the viewer (which
   verifies the session), the `known` map (pull requests are remembered after
   they leave the feed, so a merged one's detail still opens), and a cache
   write. A 401 marks the session rejected and fails the call with
   `GitHubAuthFailed`; any other failure stays in that repository's section.
5. Repositories still waiting on GitHub's merge computation get a background
   re-check per `rostrum_core::MergeProbeBudget` (2s, 4s, 8s, then quiet until
   the next full refresh), applied through the same path and delivered to the
   observer. `mergeStatesSettling` says one is pending.

Issues and stacks ride the same refresh. On a full refresh (not the
background notification check, which stays pull-requests-only) each
repository's fetch is the pull request query joined with the open-issues
query (`issuesPerRepo`, default 25), then — when it has open pull requests
— the Stacks API. A 404 there means the repository has no stacks; it is
remembered for an hour and not asked again. A failed stacks read keeps the
last answer; a failed issues read keeps the issues and fails only the
Issues tab's load. The pull request result decides staleness: a fetch whose
pull requests lost to a newer one applies nothing. Each part is cached
(`Write::Issues`, `Write::Stacks`, `Write::RepoMeta`) and hydrated with the
pull requests on the next launch.

### Sort, tabs and stacks in the snapshot

- The snapshot comes from `rostrum_core::flatten_tab(repos, filter, tab)`,
  which orders repositories by the repository sort and items by the item
  sort, a stack sorting as one unit by its bottom member. The phone does no
  ordering of its own.
- `stacks::PullItems` folds the rows: a `StackHeader` opens a
  `PullItem.Stack`, the `PrRow`s that carry a stack slot join it, anything
  else closes it. Stack titles, member counts and the merge rollup come from
  `StackGroup` and `MergeRollup`, the desktop's own.
- Sort setters go through `Config::absorb_filter` and publish; the tab
  through `Config::feed_tab`. Both are read back at open.

### Issues

- `issueDetail` fetches the issue with its comments and timeline (one
  GraphQL query), keeps it in memory, caches it, and renders it off the
  caller's thread like `pullDetail`; `cachedIssueDetail` reads memory, then
  SQLite.
- Each action is one `IssueMutation` REST call, then a re-read of that
  repository's open issues (whose failure is logged, not reported — the
  action itself succeeded), so a close drops the row from the tab.
- `createIssue` builds an `IssueDraft`: no blank title reaches GitHub
  (`InvalidInput`), repeats in the label and assignee lists collapse, a
  blank body is omitted.
- `assignableUsers` is fetched once per repository per session.

### Editing and paging

- Conversations are read newest page first (`rostrum_core::paging`). A
  detail's `hasEarlier`/`earlierCount` come from `Conversation::has_earlier`
  and `earlier_remaining`.
- `loadEarlierIssue`/`loadEarlierPull` take the held conversation (memory,
  then SQLite; `InvalidInput` if the screen was never opened), fetch only
  the connections with a cursor (`issue_earlier`, `conversation_earlier`),
  `merge_earlier` (no duplicates, chronological), and keep and cache the
  result. With nothing earlier they return the held detail without a
  request.
- `issueDetail`/`pullDetail` reload the newest page and keep the earlier
  pages already loaded (`Conversation::refreshed_by`), so a reload never
  loses what "load earlier" brought in; the caches hold the merged set and
  its cursors.
- `editIssue` builds the baseline from the held detail whose `updatedAt`
  equals `baseUpdatedAt` (`IssueEditor`); with no such copy, any change
  after `baseUpdatedAt` counts as a conflict. A blank title is
  `InvalidInput`; an unchanged edit sends nothing. Without `overwrite` the
  issue is re-read, and a title/description change since is
  `EditConflict` (the fresh copy is kept, nothing sent); otherwise one
  `PATCH`, then the issue and the repository's issues are re-read.

### Stack actions

- Every action is a `rostrum-remote` stack route on the paired desktop
  (`NotPaired` without one), which re-validates against GitHub and runs
  `rostrum-stack` on its clone. Starting answers at once with a `StackJob`;
  `stackJob(id)` polls it.
- Cheap checks run first and never reach the desktop: numbers listed once,
  at least two for make/arrange and one for extend, a valid trunk and branch
  names (`RefName`), a non-zero stack number.
- `checkStackPlan` and `stackCandidates` answer from the cached feed with
  `rostrum_core::plan_stack` / `plan_extend` — the desktop's own rules — so
  the phone can show what is eligible and what would be rewritten before
  asking. The desktop's answer is authoritative.
- A `rewrite_not_confirmed` refusal becomes `RewriteNotConfirmed(branches,
  reason)`: the core re-asks the desktop's dry run for the same request so
  `branches` are what it would rewrite now. A clone busy with another job is
  `RemoteApi(BUSY)`, as for local jobs; an unknown job id is
  `RemoteApi(NOT_FOUND)`.
- The first time a job is seen finished (from a start or a poll), the
  repository's feed is refreshed once (`CoreState::settled_stack_jobs`), so
  the observer shows the new stack, merge or rewrite.

### Repository screen

- `repoOverview` reads the feed's state for one watched repository:
  `repo_pull_rows` (pull requests and stack groups in the item sort,
  unfiltered) folded through `PullItems`, and `order_issues`. `url`, `stars`
  and `defaultBranch` come from the last branch-tree fetch, else the feed's
  repository facts and the plain GitHub URL.
- `branchTree` is the desktop's `fetch_branches`: the trunk choice from
  `config.json`, `repo_branch_meta` probing its names, `Trunks::resolve`,
  `ComparePlan` answered by one `divergences` batch, then `build_tree` and
  `rows()`. Each pull request row carries its feed row and its stack label
  (`stack N` / `chain`, from `stack_groups`). A repository with no commits
  has no rows.
- `setTrunks` validates every name with `TrunkName::parse` before writing
  (`InvalidInput` names the bad one); `None` returns to detection.

### Detail, diff and review

- `pullDetail` fetches the conversation (one GraphQL query, which also
  reports open/closed/merged), keeps it in memory and caches it;
  `cachedPullDetail` and the diff read it from memory or SQLite.
- Changed files are keyed on the head sha the feed reports: memory, then
  `rostrum_db::Db::load_pull_request_files`, then GitHub. A diff is fetched
  once per push.
- `fileDiff` highlights each side of the file as one stream (so multi-line
  constructs stay coloured within a hunk), pairs removed/added runs for
  word-level emphasis (`rostrum_diff::hunk_word_changes`), and emits rows with
  each commentable line's anchor from `DiffLine::anchor`. Threads and fresh
  drafts are placed after the line their anchor names; stale drafts are not
  placed at all.
- `addDraft` re-checks the anchor Kotlin hands back against the current
  head's diff (`review::anchor::resolve`): it must be a commentable line of
  that file on that side, and a range must stay within one hunk. The first
  draft tags the book with the head; a stale book refuses additions and
  submission with `DraftsStale`.
- Mutations make one GitHub call, then re-read the repository so the feed and
  header show GitHub's answer.

### Desktop

- Pairing checks `Hello.api_version`, exchanges the code, keeps the client
  (with its device token) in memory as the session's remote, and returns the
  secrets for Kotlin to store. A handed-over GitHub token is applied only if
  none is set.
- A re-pairing names the device it replaces: when this core already has a
  remote for a desktop with the same certificate fingerprint (paired this
  session, or restored with `setRemote`), the pair request carries that
  remote's device token as `replaces`, and the desktop drops that device's
  record once the new pairing succeeds. Otherwise `replaces` is absent and the
  desktop falls back to replacing a record with the same device name.
- Every desktop call goes through `rostrum_remote::client::RemoteClient`:
  the certificate is pinned by fingerprint, hosts are tried in order from the
  last one that answered, and a request is never re-sent to a second host.
- Local jobs, status and abort build their `PrRef` / `head_ref` from the
  known pull request; sync-all builds one `PrRef` per open pull request in
  the feed whose repository `machineInfo` lists a clone for.

### Copying the desktop's config

The desktop serves the shareable part of its `config.json`
(`rostrum_remote::DesktopConfig` on `/api/v1/config`): repositories in its
order, pull requests per repository, the four feed preferences, and
autostash. Clone paths and the conflict handler describe the desktop and are
not sent; the refresh interval and notification switches are each device's
own and are not copied; the search box is never touched.

- `desktopConfig` fetches the machine info and the config together and diffs
  the config against this phone's (`remote::config::preview`, pure):
  `added` in the desktop's order, `removed` in the phone's (an entry the
  phone cannot parse is listed as removed, since copying drops it).
  `changesAnything` is computed by applying the copy to a clone of the
  phone's `Config` and comparing the two, so it cannot disagree with what
  `copyDesktopConfig` would do — a reorder alone counts as a change, because
  it reorders the feed.
- `copyDesktopConfig` fetches the config again rather than trusting the
  preview Kotlin holds, then, inside the actor: writes it with
  `remote::config::apply` (repeats and blank authors dropped, pull requests
  per repository clamped to 1..=100, clone paths of dropped repositories
  dropped with them, as `Config::remove_repo` does), which leaves memory
  untouched if the file cannot be written; replaces the feed's repositories
  with `FeedState::set_repos` (kept repositories keep their pull requests and
  collapse state, new ones start idle); forgets everything held for dropped
  repositories through `CoreState::forget_repo`, the same cleanup
  `removeRepo` uses (pending review drafts are kept); rebuilds the filter
  from the new preferences while keeping the query; and publishes, so the
  observer sees the new feed. It returns the new `Settings`; Kotlin runs
  `refreshFeed` next.
- Both return `NotPaired` without a desktop and map the client's failures
  like every other desktop call (`DeviceRevoked`, `DesktopUnreachable`,
  `CertificateMismatch`, …).

### Notifications

`checkNotifications` refreshes every repository (without disturbing a
foreground probe cycle), folds the feed into the seen set
(`rostrum_core::Baseline`, persisted in SQLite and acknowledged before the
call returns), and turns the arrivals into events: filtered by the two
settings, never the viewer's own new pull requests, at most one per pull
request (a review request wins). A repository's first observation — and the
first check ever — is a baseline and reports nothing. `markNotificationsSeen`
folds without reporting, for when the user has looked at the feed.

### Profiles

Each profile is a complete core data directory; the registry is the list of
them.

```
<root>/profiles.json          registry: profiles, active id (no secrets)
<root>/profiles/<id>/         one core's data dir: config.json, cache.db
```

- **Records.** `ProfileInfo { id, label, kind, githubLogin?, createdAtMs,
  lastUsedMs }`. The id is 16 random hex characters (`getrandom`); Kotlin
  keys the profile's Keystore secrets by it. `kind` is `Desktop { machine,
  fingerprintShort }` or `TokenOnly`. The file stores the desktop's *full*
  certificate fingerprint (public — every pairing link carries it), which is
  what identifies a desktop across re-pairings.
- **Persistence.** Every edit is made on a copy, written to
  `profiles.json.tmp` and renamed over the file, and only then adopted in
  memory, so memory and disk never disagree and a crash leaves the old or
  the new registry, never half. A missing file is an empty registry; a
  malformed one is a `Storage` error rather than a fresh start, which would
  orphan the directories beside it. An active id naming no profile is
  dropped on load. `profiles()` is most recently used first (ties: newest
  created, then id).
- **Cores.** `core(id)` opens a profile's `RostrumCore` on first use and
  returns the same `Arc` after. The open-cores map is behind an async mutex
  held across opening and removal, and existence is re-checked under it, so
  one directory never has two cores and a profile removed meanwhile is not
  reopened. Kotlin never calls `RostrumCore.open` on a profile directory
  itself (`open` stays exported for single-core use and tests).
- **Pairing.** `pairDesktopWithLink` / `pairDesktopManual` look for a
  profile with the endpoint's fingerprint. If there is one, they pair into
  its core (`created = false`; the machine name is refreshed, the user's
  label kept). Otherwise they open a core in a fresh directory, pair it, and
  register the profile only after the pairing succeeded (label = machine
  name); on any failure the core's storage is closed and the directory
  deleted, so a failed pairing leaves nothing behind. Pairings run one at a
  time, so the same desktop paired twice at once cannot make two profiles.
  Pairing never changes the active profile. Re-pairing into a profile goes
  through that profile's core, so the request names the profile's current
  device token as `replaces` when its core is open with the remote restored
  (Kotlin restores each profile's remote at startup), and names none
  otherwise. The GitHub token handover in
  `PairingResult` works as for a single core: it is applied to that
  profile's core when it has none, and Kotlin stores it under the profile id.
- **Before any profile.** The registry's `parsePairingLink` and
  `probeDesktop` call the same functions as the core's
  (`remote::pairing_preview`, `remote::probe_desktop_at`), so the Pair screen
  can read a link and probe a desktop on first run; neither creates a profile
  or touches disk.
- **Removal.** `removeProfile` unpairs on the desktop when the profile's core
  is open with a remote (any failure — unreachable, revoked — is logged and
  ignored), closes the core's SQLite pool, deletes the directory, removes the
  record, and clears `active` if it pointed there. Kotlin deletes the
  profile's secrets and drops any `RostrumCore` it still holds.
- **Cleanup.** Opening the registry deletes directories under `profiles/`
  that are shaped like an id but registered to no profile — what a crash
  between creating a directory and registering it would leave.
- **The active profile and notifications.** The feed shows the active
  profile's core. Background notifications cover every profile: the
  WorkManager job walks `profiles()`, hands each core its token, and calls
  `checkNotifications` on each; each profile keeps its own seen set.
- **Legacy data** from the single-core layout is not migrated; Kotlin wipes
  it.

### Secrets

The GitHub token and the device token arrive from Kotlin, live in memory
(`session::Session`, inside `RemoteClient`), and are handed to the clients.
They are never written to SQLite or `config.json`, never logged (the token
types redact themselves in `Debug`; the log line on a new token carries only
its last four characters), and leave Rust only in `PairingResult` and
`DesktopGitHubToken` so Kotlin can put them in the Keystore. The tests scan
the data directory for every secret that passed through.

## Files

| File | Role | Key exports / interfaces |
|---|---|---|
| `crates/rostrum-ffi/Cargo.toml` | `cdylib` + `lib`; `uniffi-bindgen` bin; uniffi `tokio` everywhere, `cli` only off Android | — |
| `crates/rostrum-ffi/uniffi.toml` | Kotlin config: `android = true`, immutable records | — |
| `src/lib.rs` | Scaffolding and module map | `RostrumCore`, `RostrumError` |
| `src/bin/uniffi-bindgen.rs` | The bindgen (a stub on Android targets) | — |
| `src/error.rs` | The error enum and every conversion into it | `RostrumError`, `RemoteErrorCode`, `From<GitHubError/ClientError/DbError/ConfigError>` |
| `src/types.rs` | Shared records and domain conversions | `UserRef`, `Chip`, `ColorRole`, `LabelView`, `Side`, `CheckState`, `MergeStatus`, `ReviewState`, `PullState` |
| `src/engine/mod.rs` | The object, `open`, GitHub-error bookkeeping | `RostrumCore::open`, `open_with_github_api` (hidden test seam) |
| `src/engine/actor.rs` | State mailbox | `Actor::call/try_call`, `WeakActor` |
| `src/engine/writer.rs` | Ordered SQLite writes with acks | `Writer`, `Write`, `settled` |
| `src/engine/notifier.rs` | Ordered observer delivery | `Notifier` |
| `src/engine/state.rs` | `CoreState`, `PullKey`, input parsing, `publish`, `forget_repo` | — |
| `src/engine/recent.rs` | Bounded in-memory caches | `Recent` |
| `src/session.rs` | Token, trust, viewer; GitHub API root | `GitHubStatus`, `Session`, `GitHubApi` |
| `src/settings.rs` | Settings screen | `Settings` |
| `src/feed/types.rs` | Feed records and the observer trait | `FeedSnapshot`, `RepoSection`, `RepoBody`, `PrSummary`, `FeedObserver`, … |
| `src/feed/state.rs` | Feed state, fetch sequencing, `set_repos`, issues and stacks, hydration, snapshot from `flatten_tab` | `FeedState`, `Applied`, `Cached` |
| `src/feed/refresh.rs` | Fetch pipeline (pull requests, issues, stacks) and merge-state probes | `fetch_repo`, `fetch_all`, `RepoFetch`, `Scope`, `Probes`, `ProbeSlot` |
| `src/feed/chips.rs` | Chip text and colour roles (the desktop's rules) | `merge_chip`, `behind_chip`, `base_divergence`, … |
| `src/feed/summary.rs` | Feed row | `summarize` |
| `src/sort.rs` | The two sorts: records, conversions, setters | `SortSettings`, `RepoSortKey`, `ItemSortKey`, `SortDirection`, `settings`, `chosen` |
| `src/stacks.rs` | Stack header records and the row folder | `PullItem`, `StackSummary`, `StackKind`, `StackRollup`, `PullItems` |
| `src/issues/types.rs` | Issue records | `IssueSummary`, `IssueDetail`, `IssueStatus`, `IssueCloseReason`, `CloseIssueAs` |
| `src/issues/summary.rs` | Issue row and status chip | `summarize_issue`, `status_chip` |
| `src/issues/mod.rs` | Issue screen, actions, creation; held and merged detail | `held_issue`, `keep_issue`, `fetch_issue` |
| `src/issues/edit.rs` | Title/description edit with conflict check | `Baseline` |
| `src/issues/paging.rs` | Load earlier on the issue screen | — |
| `src/stack_actions/types.rs` | Stack action records and wire conversions | `StackJob`, `StackJobState`, `StackJobResult`, `StackPlanRequest`, `StackRewritePlan`, `StackPlanCheck`, `StackCandidate`, `StackMergeMethod` |
| `src/stack_actions/check.rs` | Input parsing and local plan/eligibility checks | `PlanInput`, `check`, `candidates` |
| `src/stack_actions/mod.rs` | Stack actions through the desktop, typed refusals, refresh after a job | — |
| `src/repo_view/types.rs` | Repository screen records | `RepoOverview`, `BranchTree`, `BranchRow`, `TrunkDrift`, `BranchDrift`, `TrunkSettings` |
| `src/repo_view/tree.rs` | Core branch tree → rows | `rows` |
| `src/repo_view/mod.rs` | Overview, branch-tree fetch, trunks | `parse_trunks`, `trunk_settings` |
| `src/detail/types.rs` | Detail records | `PullDetail`, `PullHeader`, `MergeVerdict`, `TimelineEntry`, … |
| `src/detail/header.rs` | Header derivation | `header` |
| `src/detail/timeline.rs` | Timeline, threads, checks | `timeline`, `thread_view`, `check_view` |
| `src/detail/actions.rs` | PR-level mutations | — |
| `src/markdown.rs` | Markdown tree → flat blocks | `MdBlock`, `MdBlockKind`, `MdSpan`, `render`, `render_markdown` |
| `src/diff/types.rs` | Files-tab records | `FilesOverview`, `FileDiff`, `DiffRow`, `CommentAnchor`, … |
| `src/diff/load.rs` | Files per head; cached conversations | `LoadedFiles` |
| `src/diff/highlight.rs` | Two-stream syntax highlighting | `file_highlights` |
| `src/diff/segments.rs` | Runs + emphasis → segments | `segments`, `argb` |
| `src/diff/rows.rs` | Diff rows with anchors, threads, drafts | `build_rows` |
| `src/diff/overview.rs` | Overview and file availability | `overview`, `changed_file`, `availability` |
| `src/review/types.rs` | Review records | `PendingReview`, `ReviewDraft`, `DraftAnchor`, `ReviewEvent` |
| `src/review/book.rs` | Drafts with stable ids, one head per book | `DraftBook`, `Draft` |
| `src/review/anchor.rs` | Anchor re-validation | `resolve` |
| `src/remote/types.rs` | Desktop records | `PairingResult`, `LocalStatus`, `JobResult`, `SyncRun`, … |
| `src/remote/convert.rs` | Protocol → records | — |
| `src/remote/refs.rs` | `PrRef` building for jobs and sync-all | `pr_ref`, `sync_refs` |
| `src/remote/config.rs` | Copying the desktop's config: preview diff, apply, exports | `DesktopConfigPreview`, `preview`, `apply` |
| `src/notifications.rs` | Notification check and event policy | `NotificationEvent`, `NotificationKind`, `events` |
| `src/profiles/mod.rs` | The registry object: cores per profile, pairing into profiles, removal | `ProfileRegistry`, `ProfileInfo`, `ProfileKind`, `ProfilePairing` |
| `src/profiles/store.rs` | `profiles.json`: records, atomic save/load, ordering, ids | `RegistryFile`, `ProfileRecord`, `new_id` |
| `src/logging.rs` | tracing → Kotlin | `LogSink`, `LogRecord`, `install_log_sink` |
| `tests/profiles.rs` | Registry persistence, ordering, switching, isolation, removal, and pairing into profiles against a TLS stand-in | — |
| `tests/remote_pairing.rs` | Pairing, every desktop call, and copying the desktop's config against a TLS stand-in | — |
| `tests/github_flows.rs` | Refresh, probes, notifications, mutations against a GitHub stand-in | — |
| `tests/stack_actions.rs` | Local checks, every stack action, `RewriteNotConfirmed`, busy, polling and the refresh after a job against the TLS and GitHub stand-ins | — |
| `tests/edit_paging.rs` | Issue edits (clear, activity-only, conflict, overwrite, invalid, unchanged) and paged issue and pull request conversations against the GitHub stand-in | — |
| `tests/issues_stacks.rs` | Tabs, sorts, stacks, the issue screen and actions, creation, the repository screen, branch tree and trunks against the GitHub stand-in | — |
| `tests/core_offline.rs` | Cache-only flows and restarts | — |
| `tests/bindings.rs` | Kotlin generation from the library this test run built (the newest `librostrum_ffi` under the profile directory, since `cargo test` does not copy it up) | — |

Logic this feature moved out of the gpui crate into gpui-free crates (the
desktop now calls these):

| Now in | What |
|---|---|
| `rostrum-core/src/review.rs` | `DraftAnchor` (range ordering, `covers`), `drafts_are_stale` |
| `rostrum-core/src/probe.rs` | `MergeProbeBudget`, `needs_merge_probe` |
| `rostrum-core/src/arrivals.rs` | Notification `Baseline` (extended with review requests, serialisable) |
| `rostrum-core/src/state.rs` | `apply_divergences`, `divergence_query` |
| `rostrum-core/src/timeline.rs` | `ReviewThread::reply_target`, `is_anchored_at`; `Conversation::state` |
| `rostrum-diff/src/parse.rs` | `DiffFile::from_patch` |
| `rostrum-diff/src/overview.rs` | `tile_heat`, `max_churn` |
| `rostrum-db/src/files.rs` | Changed files cached per head sha |

New alongside: `rostrum-diff/src/word_diff.rs` (token LCS for word
emphasis), `rostrum-db/src/baseline.rs` (the seen set),
`GitHubClient::viewer` and `with_endpoints`, `MergePullRequest` (title,
message, expected sha), `Config::try_add_repo` / `AddRepoError`, and
`Config::notify_review_requests`.

## Generating the Kotlin

```sh
cargo build -p rostrum-ffi
cargo run -p rostrum-ffi --bin uniffi-bindgen -- generate \
    --library target/debug/librostrum_ffi.so --language kotlin --out-dir out/
```

Library mode reads the metadata from the library's symbol table, so no
build profile may strip symbols. `tests/bindings.rs` runs exactly this
against the library `cargo test` builds and checks the surface.

## Invariants

- **One core per profile directory.** Cores come from `ProfileRegistry::core`,
  which opens each once and never reopens a removed profile.
- **A profile exists only after its pairing succeeded**, and a failed or
  interrupted creation leaves no directory behind.
- **`profiles.json` holds no secret**, and is always a complete file.
- **Copying the desktop's config touches only what it names.** Repositories,
  pull requests per repository, the four feed preferences and autostash; the
  refresh interval, notification switches, search query and pending drafts
  are the phone's and stay.
- **No secrets persisted in Rust.** Tokens live in memory only, are redacted
  in `Debug`, and never reach SQLite or `config.json`.
- **Anchors are computed in Rust only.** Every diff line's anchor comes from
  `DiffLine::anchor` on that line; `addDraft` accepts only anchors present in
  the current head's diff, ranges only within one hunk on one side.
- **Drafts are never lost and never misplaced.** Each change is on disk
  before the call returns; a book holds drafts for one head; a stale book is
  neither shown in the diff, extended, nor submitted.
- **Settings in memory match settings on disk.** A setter writes the file
  first and applies the change only if the write succeeded.
- **An older fetch never overwrites a newer one**, per repository.
- **One request per host.** Desktop calls go through `RemoteClient`, which
  moves to another address only when one cannot be connected to, and never
  re-sends a request that reached a host.
- **UniFFI is pinned** at `=0.32.1` in the root `Cargo.toml`; the bindgen
  binary is built from the same pin, so the Kotlin and the scaffolding
  always match.
- **Android-buildable.** `ring` for TLS, bundled SQLite, no subprocesses; the
  bindgen's `cli` feature is compiled only for the host.
- **A stack is one item.** Its members travel inside `PullItem.Stack`, so
  no sort, filter or Kotlin code can separate them from their header; a
  filter narrows the members, never the grouping.
- **History is never rewritten without the user's confirmation.** Arrange
  and extend send exactly the branches the user was shown; the desktop
  refuses any mismatch.
- **The phone orders nothing.** Repository, item and stack order are
  `rostrum_core::sort`'s, shared with the desktop; the sort survives
  `clearFilter`.
- **Background checks stay cheap.** The notification check reads pull
  requests only; issues and stacks are fetched on full refreshes.
- **No exported method is named `close` or `destroy`**, which UniFFI's
  Kotlin objects reserve for releasing the Rust side.
