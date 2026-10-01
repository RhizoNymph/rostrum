# Feature: android_issues_repo

The Android app's second round of feed features, over the core's
`feat/android-core-2` API: the feed's **sort**, the **Issues** tab and the
issue screens, **stacks** shown together in the feed, and a repository's own
**screen** with its pull requests, issues and branch tree. Everything is per
active profile (each profile has its own core, so its own sort, tab and
trunks).

## Scope

- **Sort**: a Sort button in the feed header and a bottom sheet with two
  sections, Repositories and "Pull requests & issues"; each is the core's
  keys as radio rows plus a direction toggle named by the chosen key. The
  tab bar shows the core's `summary`.
- **Tabs**: Pull requests | Issues under the header with the core's
  `tabCounts`, persisted through `setFeedTab`.
- **Issues**: issue rows (status chip, labels, assignees, comments); the
  issue screen (header, labels and assignees edited through pickers,
  timeline with the new events, composer, close as completed or not
  planned, reopen); the new-issue form (repository, title, body with
  Write/Preview, labels, assignees), opened from a FAB on the Issues tab or
  the repository screen.
- **Stacks**: `PullItem.Stack` drawn as a header row (title, trunk, members
  not open, rollup chip) above its members, bottom first, each indented
  behind a chain glyph. The header's menu runs stack actions on the paired
  desktop: Merge stack, Add pull requests, Unstack (a GitHub stack), Make
  stack (a detected chain); Arrange picks pull requests on the repository
  screen. Every action asks first and is then followed as a job.
- **Editing an issue** (title and description, Write/Preview) with the
  conflict check, and **"Load earlier"** on issue and pull request
  conversations.
- **Repository screen**: tapping a repository header's name opens it, with
  tabs for Pull requests, Issues and Branches; Back returns to the feed.

## Non-scope

- Issues in background notifications (the core's check reads pull requests
  only).
- Any ordering on the phone: the core orders repositories, items and stacks
  (`rostrum_core::sort`), the app draws what it gets.

## Data and control flow

### Model and backend

`data/model/`: `Sort.kt` (`SortSettings`, `RepoSortKey`, `ItemSortKey`,
`SortDirection`, `SortOption<K>` with `directionLabel`), `Stacks.kt`
(`StackSummary`, `StackKind` = `GitHub(number)` | `Chain`, `StackRollup`),
`Issues.kt` (`IssueSummary`, `IssueRef` — distinct from `PrRef` —,
`IssueStatus` = `Open` | `Closed(reason?)`, `IssueCloseReason`,
`CloseIssueAs`, `IssueDetail`), `RepoView.kt` (`RepoOverview`, `BranchTree`,
`BranchRow` = `Trunk` | `OtherBases` | `Base` | `Pull`, `TrunkDrift`,
`BranchDrift`, `BranchNote`, `TrunkSettings`). `Feed.kt`: `FeedSnapshot`
gains `tab`, `tabCounts`, `sort`; `RepoBody.Pulls(items: List<PullItem>)`
(with `pulls`, every listed pull request flattened) and
`RepoBody.Issues(issues)`; `PullItem` = `Single(pull)` |
`Stack(stack, members)`; `AuthorChip.openItems`. `TimelineEvent` gains
`ClosedAs(reason)`, `Unassigned` and `CrossReferenced(source, title)`.

`RostrumBackend` gains `setFeedTab`, `sortSettings`, `setRepoSort`,
`setItemSort` (a `null` direction lets the core choose: a new key starts at
its default, the same key keeps its direction), the issue calls
(`issueDetail`, `cachedIssueDetail`, `commentOnIssue`, `closeIssue`,
`reopenIssue`, `add/removeIssueLabel`, `assignableUsers`,
`add/removeIssueAssignee`, `createIssue` → the new number), and the
repository calls (`repoOverview`, `branchTree`, `trunks`, `setTrunks`).
`FfiRostrumBackend` maps each to one core call (`SortMappings.kt`,
`IssueMappings.kt`, `RepoViewMappings.kt`).

The fake mirrors the core: `FakeSort` (key kinds, defaults, labels,
`orderRepos`, `orderItems` with a stack as one unit — text keys by its
bottom member, time keys by its newest member descending or oldest
ascending), `FakeStacks` (sample GitHub stack 7: rostrum #9 with #11 on
top; `fold` puts a stack where its first visible member was), `FakeIssues`
(sample issues, timelines, every action and the events it records),
`FakeRepoView` (overview, branch tree, trunk validation),
`FakeFeedAssembler` (per tab, filters both kinds alike).

### Feed

`FeedViewModel.selectTab` → `setFeedTab` (and the filter sheet's roster
reloads, since it counts the active tab's authors). `openSort`/`closeSort`
toggle `FeedUiState.sortOpen`; `chooseRepoSort(key)` /
`chooseItemSort(key)` call the setters with no direction;
`setRepoSortDirection` / `setItemSortDirection` pass the current key and
the chosen direction. Every answer is the next snapshot, applied as usual.

`FeedList` turns each `RepoSection` into lazy segments: header, stale
notice, then `ItemRow`s (`ui/items/ItemRows.kt`: `rowsOf` expands a stack
into `StackHeader` + `Pull(stack = StackPlace)` rows; issues become
`ItemRow.Issue`), drawn by `ItemRowContent`. The header's name opens the
repository screen; the chevron still collapses.

### Stack actions (`ui/stacks/`)

`StackActionsViewModel(backend, session)` — one per feed and per repository
screen — holds `flow: StackFlow?` and the Arrange `selection`:

- `request(entry, repo, stack, members)` from a header's menu
  (`ui/items/StackHeaderRow.kt`, `menuEntries`: a GitHub stack offers Merge,
  Add, Unstack; a chain offers Make). Without a paired desktop it becomes
  `NeedsDesktop`, whose dialog links to pairing.
- **Merge**: `ConfirmMerge` lists the members with their merge chips, a
  Merge/Squash/Rebase toggle and the all-or-nothing note → `mergeStack`.
  **Unstack**: a confirmation → `unstack`. **Make**: the chain bottom first
  on its trunk → `makeStack` (nothing rewritten).
- **Add**: `PickExtend` loads `stackCandidates` (eligible ones pickable,
  in pick order, each with its note) → `planStackRewrite(Extend)` →
  `ConfirmRewrite` showing exactly the branches the desktop names →
  `extendStack(…, confirmRewrite = those branches)`.
- **Arrange** (repository screen, Pull requests tab): picking mode on the
  rows (`RowCallbacks.picked`, a numbered badge), then `OrderArrange` (move
  up/down, trunk, `checkStackPlan` re-run on each change) →
  `planStackRewrite(Arrange)` → `ConfirmRewrite` → `arrangeStack`.
- A `RewriteNotConfirmed` refusal turns the open `ConfirmRewrite` into the
  desktop's new branches with its reason, to confirm again; `RemoteApi(BUSY)`
  is a snackbar with the question kept; `NotPaired` is `NeedsDesktop`.
- A started job is `StackFlow.Job`, polled with `stackJob` every second
  until finished; the sheet shows progress, then the outcome (`jobOutcome`):
  a handed-off conflict with its session and `tmux attach -t …`
  (`CopyCommandRow`), a failure with the pull requests already pushed. The
  outcome also goes to the snackbar; closing the sheet doesn't stop the job.

### Editing and paging

- `IssueViewModel.openEditor(field)` starts an `IssueEditor` from the issue
  as shown, its `updatedAt` the base. `saveEdit` → `editIssue(…, base,
  overwrite = false)`; `EditConflict` keeps the draft and opens the conflict
  dialog: **Reload** drops the draft and re-reads the issue, **Overwrite**
  resends with `overwrite = true` over GitHub's `updatedAt`. A blank title
  can't be saved; Preview renders with `renderMarkdown`.
- `loadEarlier()` on the issue and pull request ViewModels merges the next
  earlier page (`loadEarlierIssue` / `loadEarlierPull`); the row ("Load
  earlier (N more)", `components/LoadEarlierRow.kt`) sits after the
  description, where earlier entries come in.

### Issue screen (`ui/issue/`)

`IssueViewModel(backend, issue, clock)`: paints `cachedIssueDetail`, then
`issueDetail` (a failed fetch keeps what is shown). Comment, close (two
reasons), reopen, and each label or assignee toggle call the backend, then
re-read the issue, so header and timeline show GitHub's state. Pickers load
`repositoryLabels` / `assignableUsers` on open, apply one change at a time
(`PickerState.pending`). `stateActions(issue)` gives the menu: close as
completed / not planned while open, reopen once closed.

### New issue (`ui/newissue/`)

`NewIssueViewModel(backend, presetRepo, appMessages)`: the watched
repositories from `settings()`; the preset (from a repository screen), or
the only one. Labels and assignees are picked locally and dropped when the
repository changes. Preview renders the body with `renderMarkdown`.
`submit` → `createIssue`; success says "Opened #N in repo" through the app
messages and sets `created`, on which the route replaces the form with the
new issue's screen.

### Repository screen (`ui/repo/`)

`RepoViewModel(backend, repo, clock)`: `repoOverview` (no network) on open
and on every `feedUpdates` emission; `branchTree` the first time Branches
is shown; pull to refresh = `refreshRepo` then both again. The trunk editor
opens from the tree's `TrunkSettings` (or `trunks()`), toggles Detect /
Custom, and saves `setTrunks(repo, null)` or the parsed names
(`parseTrunkNames`: commas and whitespace, repeats dropped); the core
validates each name, and an `InvalidInput` stays in the editor. Branch rows
indent by depth; pull request rows open the pull request.

## Files

| File | Role / key exports |
|---|---|
| `data/model/Sort.kt`, `Stacks.kt`, `Issues.kt`, `RepoView.kt` | The records above |
| `data/model/Feed.kt` | `FeedTab`, `TabCounts`, `PullItem`, `RepoBody.Pulls`/`Issues` |
| `data/ffi/SortMappings.kt`, `IssueMappings.kt`, `RepoViewMappings.kt` | Generated records ↔ model |
| `data/fake/FakeSort.kt`, `FakeStacks.kt`, `FakeIssue.kt` (`SampleIssues`), `FakeIssues.kt`, `FakeRepoView.kt`, `FakeFeedAssembler.kt`, `SampleDrafts.kt` | The fake's sort, stacks, issues, repository screen, assembly |
| `ui/items/ItemRows.kt` | `ItemRow`, `rowsOf`, `StackPlace`, row chips and texts (moved from the feed) |
| `ui/items/PrRow.kt`, `IssueRow.kt`, `StackHeaderRow.kt` (`StackFlags`), `ItemRowContent.kt` | Rows shared by the feed and the repository screen |
| `ui/feed/FeedTabs.kt`, `FeedSortSheet.kt` | Tabs with counts and the sort summary; the Sort sheet |
| `ui/issue/IssueState.kt`, `IssueViewModel.kt`, `IssueScreen.kt`, `IssueRoute.kt` | The issue screen |
| `ui/newissue/NewIssueState.kt`, `NewIssueViewModel.kt`, `NewIssueScreen.kt`, `NewIssueRoute.kt` | The new-issue form and its repository picker |
| `ui/repo/RepoState.kt`, `RepoViewModel.kt`, `RepoScreen.kt`, `RepoRoute.kt` | The repository screen, branch tree, trunk editor |
| `ui/stacks/StackFlow.kt`, `StackActionsViewModel.kt`, `StackActionSheets.kt` | Stack action flow states and texts, the ViewModel, the sheets and dialogs (`StackActionsHost`) |
| `ui/issue/IssueEditSheet.kt` | The title/description editor and the conflict dialog |
| `ui/components/LoadEarlierRow.kt` | "Load earlier (N more)" |
| `data/IssuesApi.kt`, `data/StackActionsApi.kt` | The issue and stack-action parts of `RostrumBackend` |
| `data/model/StackActions.kt`, `data/ffi/StackActionMappings.kt` | Stack action records and their mappings |
| `data/fake/FakeHost.kt`, `FakeIssuesApi.kt`, `FakeStackActions.kt` | The fake's delegated issue API and stack actions (plans from base/head chains, refusals, polled jobs) |
| `ui/components/PickerSheet.kt` | `PickerSheet`, `PickerOption`, `PickerKind`, `PickerState` |
| `ui/components/TimelineItems.kt`, `CommentBar.kt`, `CardSegment.kt`, `NewIssueFab.kt` | Shared by pull requests, issues and the repository screen |
| `ui/navigation/Destinations.kt` | `Issue(repo, number)`, `NewIssue(repo?)`, `Repo(repo)` |

Paths are under `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/`.
Tests: `data/fake/FakeSortTest`, `FakeIssuesStacksTest`,
`data/ffi/FfiMappingsTest` (sort, issues, stacks, repository records and
new events), `HostSmokeTest` (sorts, tab, overview, trunk validation,
blank-title refusal against the real core), `ui/feed/FeedTabsSortTest`,
`ui/items/ItemRowsTest`, `ui/issue/IssueViewModelTest`, `IssueEditTest`,
`ui/pr/PrLoadEarlierTest`, `ui/stacks/StackActionsViewModelTest`,
`data/fake/FakeStackActionsEditTest`,
`ui/newissue/NewIssueViewModelTest`, `ui/repo/RepoViewModelTest`.

## Invariants

- **The phone orders nothing.** Repositories, items and stacks arrive in the
  core's order; the app never sorts.
- **A stack is one item.** Its members travel inside `PullItem.Stack` and
  are drawn together under its header; a filter narrows members, never the
  grouping.
- **Sort keys come from the core's options**, so the sheet can only offer
  valid keys, and a new key resets to its default direction.
- **After an issue action the screen re-reads the issue**; nothing is
  patched locally.
- **The new-issue form sends nothing until Create**; labels and assignees
  belong to the chosen repository and are dropped when it changes.
- **History is rewritten only as confirmed.** Arrange and Add send exactly
  the branches the desktop's dry run named and the user saw; a different
  set is refused and shown again.
- **Every stack action asks first** and needs the paired desktop.
- **An edit never silently overwrites**: a change on GitHub since the edit
  began is a conflict the user resolves.
