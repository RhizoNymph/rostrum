# Feature: android_ci_config

The Android app's third round over the core's `feat/android-ci-config` API:
**sending this phone's settings to the paired desktop** (the reverse of
copying them, guarded by the desktop's revision), an **issues per
repository** setting, and the **CI grid**: every open pull request's checks
in one two-way scrolling matrix, with job logs, another app's output, and
confirmed re-runs. Everything is per active profile.

## Scope

- **Send settings to <machine>**: a row in the Desktop tab ("Settings"
  section) and in Settings › Desktop. A sheet previews the core's
  `pushChanges` (one row per setting: a list's additions and removals, or a
  value's before → after) with Cancel and Send. Send names the previewed
  `revision`; `Changed` (or a `CONFIG_CHANGED` refusal) shows "The desktop's
  settings changed since you looked" with the fresh difference and "Send
  again". An empty difference shows "Nothing to send".
- **Copy settings** (from the earlier round) now names issues per
  repository, sorts and trunks in its change line and summary.
- **Settings › Fetching**: pull requests and issues per repository, each a
  choice of 10/25/50/100 (plus the current value).
- **CI grid** (`Destination.Checks(repo?)`): from a Checks icon in the feed
  header (every repository) and in the repository screen's top bar (that
  repository only). A frozen left column (number, title, rollup) beside
  per-repository column headers and cells that scroll horizontally
  together; cells are status tiles (glyph, status, timing label) tinted by
  the core's role. "Needs attention" filter chip, pull to refresh, a
  one-second rebuild while `ticks`, a 15-second fetch while `anyRunning`.
- **A cell's sheet**: an Actions job's log (monospace lazy list,
  collapsible groups starting from the core's `collapsed`, the failing step
  tinted, search with previous/next, "First error", "Load full log" when
  truncated); another app's output (title, summary and text as markdown,
  annotations); a legacy status's "Open in browser". Every kind has a
  Retry menu from `rerunTargets` with the core's confirmation prompt.

## Non-scope

- Choosing what to push: a push sends this profile's whole shareable
  settings; the desktop writes only those keys.
- Building the grid, timing labels, log parsing, re-run eligibility and the
  optimistic "queued" flip: all the core's. The phone only draws them and
  decides when to ask again.
- Re-running from the pull request's Checks tab (it still links out).
- Push or copy without a paired desktop (both answer `NotPaired`).

## Data flow

### Sending settings

```
Desktop tab / Settings › Desktop ── "Send settings to framework"
  └─ PushSettingsViewModel.open()
       Loading → backend.desktopConfig() → Ready(preview)          (or Failed → Retry)
         preview.pushChanges.map(ConfigChangeText::view) → ConfigChangeList
  Send ─ pushConfigToDesktop(preview.revision)
       Applied(desktop)  → Closed, snackbar "Sent settings to framework"
       Changed(desktop)  → Ready(desktop, stale = true): banner + fresh diff + "Send again"
       Err(RemoteApi(ConfigChanged)) → re-read desktopConfig() → Ready(stale = true)
       Err(other)        → Ready(send = Failed(error)), sheet stays open
  Cancel / dismiss ─ Closed; nothing is sent (dismiss is ignored while sending)
```

`ConfigChangeText.view` turns the core's strings into a `ConfigChangeView`:
list fields (repositories, authors, trunks; `a, b` or `(none)`) become
`ListDiff(added, removed, reordered)`; everything else becomes `Value` with
`true`/`false` read as on/off and `(unset)` as "not set".

### The CI grid

```
CiRoute(repo?) ─ CiGridViewModel(backend, repo)
  init: ciGrid(filter)              (held checks, no network; shown at once)
        refreshCi(filter) | refreshCiRepo(repo, filter)   (refreshing = true)
        each result → CiGridLayout.only(grid, repo) → content
  LifecycleStartEffect: start() … stop()
        every tickMillis (1 s):  if grid.ticks      → ciGrid(filter)
        every pollMillis (15 s): if grid.anyRunning → fetch again
  Needs attention ─ setNeedsAttention(on) → ciGrid(new filter)
  Pull to refresh ─ refresh() → fetch (one at a time)
  A failed fetch with a grid on screen → snackbar; without one → ErrorView
```

`CiGridTable` draws `grid.lines` in one `LazyColumn`: `Header` (repository
name, then "Pull request" over the frozen column and the section's column
labels), `Stack` ("Stack · N PRs"), `Row` (frozen left part opens the pull
request on its Checks tab; cells open the sheet), `Notice`
(`CiGridLayout.notice`: not fetched / fetching / failed / nothing open /
nothing needs attention), `Spacer`. One `ScrollState` per repository is
shared by that section's header and rows, so they scroll sideways together.

### A cell

```
openCell(section, row, column)        (a not-run cell opens nothing)
  CiSource.Actions(jobId) → CiDetail.Log   → jobLog(repo, jobId, full = false)
                                             view.collapsed = log.collapsed
  CiSource.App(checkRunId) → CiDetail.Output → checkOutput(repo, checkRunId)
  CiSource.Status          → CiDetail.Status (detailsUrl only)
Log: toggleGroup · setLogQuery → matches, first one revealed (group unfolded) and focused
     nextMatch / previousMatch (wrapping) · jumpToFirstError
     loadFullLog → jobLog(full = true) (the core re-parses its kept raw text)
Retry… ─ RetryState.Loading → rerunTargets(repo, pr, column.key)
           Menu(Available(options)) → chooseRerun(option) → Confirm(option): ConfirmDialog(label, confirmPrompt)
              confirmRerun → Running → rerun(repo, option.rerun)
                 Ok  → sheet closes, "Asked GitHub to re-run failed jobs on #10", ciGrid (the core shows it queued)
                 Err → Failed(error): CiNoPermission / CiNotRerunnable / CiNotFound described in place
           Menu(Unavailable(reason, message)) → the message and Close
           dismissRetry → Closed (ignored while Running)
```

`LogLayout.rows(log, collapsed)` flattens lines into `LogRow.Line(index)`
and `LogRow.Group(group, collapsed, hidden)` (a folded group hides its
lines); `LogView.focus` carries a serial so asking for the same line twice
scrolls twice.

## Files

| File | Role | Key exports |
|---|---|---|
| `data/model/Ci.kt` | CI records | `CiGrid`, `CiSection`, `CiRow`, `CiCell`, `CiSource`, `CiLine`, `CiJobLog`, `CiCheckOutput`, `CiRerun`, `CiRerunChoice`, `CiGridFilter` |
| `data/model/Remote.kt` | Preview gains `revision`, `issuesPerRepo`, `copyChanges`, `pushChanges` | `ConfigField`, `ConfigChange`, `ConfigPushResult` |
| `data/model/Session.kt` | `Settings.issuesPerRepo` (default 25) | `Settings` |
| `data/CiApi.kt` | The CI half of the backend | `CiApi` |
| `data/DesktopConfigApi.kt` | Copy and push (moved out of `RostrumBackend`) | `DesktopConfigApi` |
| `data/RostrumBackend.kt` | Extends both; `setIssuesPerRepo` | `RostrumBackend` |
| `data/BackendError.kt` | `CiNoPermission`, `CiNotRerunnable`, `CiNotFound`, `RemoteErrorCode.ConfigChanged` | `BackendError` |
| `data/ffi/CiMappings.kt` | Generated CI records ↔ model | `toModel`, `toFfi` |
| `data/ffi/FfiDesktopConfig.kt` | Preview, changes, push result | `toModel` |
| `data/ffi/FfiRostrumBackend.kt` | The new calls | — |
| `data/fake/FakePhoneSettings.kt` | The fake phone's settings, shared by its parts | `FakePhoneSettings` |
| `data/fake/FakeDesktopConfig.kt` | nymph-desk's settings and revision; copy, push, `changeElsewhere` | `FakeDesktopConfig` |
| `data/fake/FakeCi.kt`, `SampleCi.kt` | The grid over the fake's open pull requests; logs, output, re-runs | `FakeCi`, `SampleCi` |
| `ui/desktopconfig/ConfigChangeText.kt` | Change rows and the push sheet's words | `ConfigChangeView`, `ConfigChangeText`, `PushConfigText` |
| `ui/desktopconfig/PushSettingsViewModel.kt` | The push sheet's state | `PushSheetState`, `PushSheetActions`, `PushSettingsViewModel` |
| `ui/desktopconfig/PushSettingsSheet.kt` | The push sheet | `PushSettingsSheet`, `ConfigChangeList` |
| `ui/desktopconfig/DesktopConfigText.kt` | Copy lines name issues, sorts, trunks | `DesktopConfigText` |
| `ui/settings/SettingsSections.kt`, `SettingsScreen.kt`, `SettingsViewModel.kt`, `SettingsModels.kt` | Fetching section, Send row, choice dialog | `FetchSection`, `FetchLimitChoices` |
| `ui/desktop/DesktopScreen.kt`, `DesktopRoute.kt` | "Settings" section with the Send row; hosts the push sheet | — |
| `ui/ci/CiGridState.kt` | Screen, sheet, log view and retry states | `CiGridUiState`, `CiSheet`, `CiDetail`, `LogView`, `RetryState`, `CiGridActions` |
| `ui/ci/CiLayout.kt` | Pure helpers | `CiGridLayout`, `CiText`, `LogRow`, `LogLayout` |
| `ui/ci/CiGridViewModel.kt` | Loading, ticking, polling, cells, logs, re-runs | `CiGridViewModel` |
| `ui/ci/CiGridScreen.kt` | Route and screen | `CiRoute`, `CiGridScreen` |
| `ui/ci/CiGridTable.kt` | The two-way scrolling matrix | `CiGridTable` |
| `ui/ci/CiCellSheet.kt` | A cell's sheet: header, Retry, output | `CiCellSheet` |
| `ui/ci/CiLogView.kt` | The log viewer | `CiLogView` |
| `ui/navigation/Destinations.kt`, `RostrumNavHost.kt` | `Destination.Checks(repo?)` | — |
| `ui/feed/FeedHeader.kt`, `FeedActions.kt`, `FeedRoute.kt` | The Checks icon | — |
| `ui/repo/RepoScreen.kt`, `RepoRoute.kt` | The Checks icon (this repository) | — |

Tests: `ui/desktopconfig/PushSettingsTest.kt`, `ui/ci/CiLayoutTest.kt`,
`ui/ci/CiGridViewModelTest.kt`, `data/fake/FakeCiTest.kt`,
`ui/settings/FetchLimitsTest.kt`, `data/ffi/CiConfigMappingsTest.kt`, and
`HostSmokeTest` order 13 (real core: issues-per-repo clamping, push input
and `NotPaired`, an empty grid without a token, `NotSignedIn` for fetches,
logs, output and re-runs, `InvalidInput` for an unfetched cell's re-runs).

## Invariants

- **A push never overwrites what the user did not see.** Send always names
  the previewed revision; `Changed` and `CONFIG_CHANGED` show the fresh
  difference and require another Send.
- **Cancel sends nothing**, and the sheet cannot be dismissed mid-send.
- **The grid ticks and polls only while the screen is started**
  (`LifecycleStartEffect` → `start`/`stop`), and only when the core says it
  is worth it (`ticks`, `anyRunning`). At most one fetch runs at a time.
- **Rebuilding is local.** The one-second loop calls `ciGrid` only; only
  pull to refresh, opening and the 15-second poll touch the network.
- **A re-run is always confirmed** with the core's `confirmPrompt`; the
  phone never flips a cell itself (the core does, and puts it back on a
  refusal).
- **A not-run cell opens nothing**, and a row's frozen part opens the pull
  request instead.
- **Opened from a repository, the grid shows only that repository** and
  fetches only it (`refreshCiRepo`).
