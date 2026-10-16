# Feature: ci_grid

A grid of CI checks for every open pull request: status, how long each check
has been running or how long since it finished, its log, and re-running it.

## Scope

- One PRs × checks matrix over the whole window, reached from the feed:
  rows are every open, non-draft pull request across all watched
  repositories, columns the union of their check keys.
- Each cell: a status tile (queued, in progress, success, failure, cancelled,
  skipped, neutral, timed out, action required, or "not run"), the live
  elapsed time while running or the time since it finished, and the duration
  on hover.
- Row headers: the pull request's repository and number (`owner/name #123`),
  title, short head sha, and a rollup.
- One header row for the whole grid: workflow names over their jobs.
- A failing-or-running filter.
- Fetching the checks with the feed's poll, and every 15 s while the grid is
  open and something is running.
- Logs: an Actions job's log in a searchable viewer with collapsible
  sections, the failing step highlighted, ANSI removed, and long logs cut to
  their tail with "load full". Another app's check: its output and
  annotations. A legacy status: its link.
- Re-runs: re-run job, re-run failed jobs and re-run all for Actions; and
  re-request for other apps' check suites. Each needs confirmation, the cells
  flip to queued at once, and errors are typed.

## Non-scope

- **All attempts.** Only the head commit's current rollup is fetched. A
  per-check history of attempts needs one REST listing per workflow run
  (`GET /actions/runs/{id}/attempts/{n}/jobs`), which is not cheap across a
  grid, so the toggle is not built. Several runs of the same check on one
  commit (a re-run, or two events triggering one job) are reduced to the
  latest, by `PrChecks::latest`.
- More than 100 contexts on one commit. The pull request is marked
  `truncated` and its row says "more not shown"; paging the contexts is
  deferred.
- Rendering ANSI colours. They are stripped; lines are coloured by their
  workflow-command kind (error, warning, notice, debug, command, group)
  instead.
- Cancelling runs, approving pending deployments, downloading artifacts.
- Drafts. They never appear, whatever the feed's draft toggle says.
- A per-repository layout, or a toggle between it and the matrix. The
  matrix replaced it; the user declined a toggle.
- Android's drawing. The rules are pure, in the shared crates, for it to
  reuse; see [For Android](#for-android).

## Layout decision: one global matrix

Earlier the grid was one section per repository, each with its own columns.
It is now one matrix: every row is a pull request from any watched
repository, and the columns are the union of check keys across all of them.
Watched repositories rarely share check names, so many cells are "not run"
— a check one repository runs reads as "not run" on every other
repository's rows. That is accepted: one header, one column order and one
selection that moves straight down across repositories read more easily
than a stack of differently shaped tables. There is no toggle back.

Columns are keyed by `CheckKey { workflow, name }` (an Actions job's workflow
plus job name; no workflow for apps and statuses), because two workflows
routinely both have a `build` job. They are ordered by workflow then name —
apps' checks and statuses (no workflow) first, then each workflow's jobs side
by side — and grouped into `ColumnGroup`s, runs of adjacent columns sharing a
workflow, so the header names each workflow once above its jobs.

### Which pull requests are rows

- **Drafts are always out**, regardless of the feed's "hide drafts" toggle:
  a draft's checks are not what the grid is for, and leaving them in would
  widen the matrix with columns only drafts run. A draft's checks add no
  columns.
- **The feed's search and author selection still apply** (including "waiting
  on" when it is on). They are what the user has narrowed the feed to, and
  the grid is reached from the feed as its checks spelled out.
- **Collapse is ignored**: collapsing a repository in the feed does not hide
  it in the grid, which is where its checks are read.
- The grid's own **failing-or-running** filter (`GridFilter`) narrows
  further.

### Order

Rows follow the feed: its repository sort, then its item sort within each
repository, with stacks grouped as the feed groups them (a "Stack · N PRs"
line heading each stack, a ┏ ┃ ┗ mark on members). Grouping by repository
keeps a repository's rows together, which is what makes stack marks still
read correctly — a stack never crosses repositories — and keeps the feed's
order familiar. A repository's first row is ruled off from the previous
repository's last with a heavier top border.

### Header and scrolling

One header row sits above the list, outside it: a corner with "N pull
requests · M checks · K hidden", then a workflow band (each `ColumnGroup`'s
name spanning its columns) over a job-name band. The list scrolls
vertically on its own and the header stays put.

Horizontal scrolling is the view's own (`HScroll`, in
`crates/rostrum/src/ci/hscroll.rs`), not an `overflow_x_scroll` container:
the row-header column is fixed at the left of every row, and the check
columns to its right — in every row and in the header — are drawn shifted
left by the same offset and clipped. So the row headers stay visible and
each column header stays over its cells while scrolling either way. A
horizontal wheel or trackpad delta (or shift with a vertical wheel) moves
the offset, clamped to the columns' width; selecting a cell from the keyboard
scrolls the least that shows its column. Because the view owns the offset,
each row draws only the columns in view plus one either side
(`HScroll::visible`), so a matrix with a hundred columns costs no more per
frame than one that fits.

The columns' viewport is the window's width less the row-header column, the
log pane when open, and a small gutter: the CI view spans the window, so this
is known at render time without measuring.

## Data

`CI_CHECKS` (`crates/rostrum-github/src/ci/wire.rs`): one request per
repository, the open pull requests (the feed's `prs_per_repo` page, most
recently updated first) with their head commit's
`statusCheckRollup.contexts(first: 100)`:

- `CheckRun`: `databaseId`, `name`, `status`, `conclusion`, `startedAt`,
  `completedAt`, `detailsUrl`, and `checkSuite { databaseId app { name slug }
  workflowRun { databaseId runNumber runAttempt workflow { name } } }`.
- `StatusContext`: `context`, `state`, `createdAt`, `targetUrl`,
  `description`.

Captured live at cost 1 for microsoft/vscode, python/cpython and
kubernetes/kubernetes (`crates/rostrum-github/fixtures/ci/`).

Decoding (`ContextNode::into_domain`) gives a `CheckEntry`:

- A check run whose suite has a workflow run is `CheckSource::Actions { job_id,
  run_id, run_attempt, suite_id }` — its check-run id *is* the job id the logs
  and re-run endpoints take. Any other check run is `CheckSource::App {
  check_run_id, suite_id, app }`. A status is `CheckSource::Status`.
- `CheckStatus::from_check_run(status, conclusion)` folds the pair into one
  word: anything not completed is queued or in progress; `STARTUP_FAILURE` is
  a failure; `STALE` reads as cancelled; an unknown conclusion reads as
  neutral. `CheckStatus::from_status_context(state)` maps pending to in
  progress and error to failure.
- A status records one time, `createdAt`: it is the start while pending and
  both start and finish once settled, so a finished status shows "finished
  2h ago" with no duration.

The store (`crates/rostrum/src/sync/ci.rs`) holds a `CiChecks`, one
`RepoChecks { prs, load }` per repository. `Store::refresh_ci` is called by
`refresh_all`, so the feed's poll refreshes checks too, with its own overlap
guard (`pending_ci`). While the CI view is visible, `set_ci_visible(true)`
fetches every repository once and starts a 15 s timer
(`CI_WATCH_INTERVAL`) that re-fetches only the repositories with something
queued or running; hiding the view drops the timer.

## The grid

`build_grid(repos, feed_filter, checks, GridFilter) -> CiGrid`
(`crates/rostrum-core/src/ci/grid/build.rs`):

1. Flatten the pull request feed with collapse cleared and `hide_drafts`
   forced on (`rows_filter`: the feed's search, authors and sort, drafts
   out), which gives the feed's order, its stack slots, and the
   repositories it shows.
2. The columns are the union of `PrChecks::latest()` keys over every one of
   those pull requests, computed before the grid filter so filtering never
   shifts columns; `group_columns` splits them into `ColumnGroup`s.
3. Each row: its repository, one cell per column (`None` is "not run"), the
   rollup over the latest entries, the short sha, the stack place, whether
   checks were fetched at all, and whether they were truncated.
4. `GridFilter { needs_attention }` keeps rows whose rollup is failing or
   running, counting the rest in `CiGrid::hidden` and per repository.
5. Each repository the feed shows becomes a `GridRepo { repo, load, rows,
   hidden }`: its checks' load state (`Idle` until first fetched) and row
   counts. With no sections, this is where a failed fetch is reported — the
   view lists failed repositories in a strip above the grid.
6. The rows are flattened into `GridLine`s — `Stack { members }` and `Row {
   row }` — for one virtualized `list`. The header is not a line.

`CiGrid` keeps its fields private and is built only by `build_grid`, so every
row's cells are exactly as long as `columns()` and the groups always partition
the columns. Accessors: `columns`, `column_groups`, `rows`, `row`, `repos`,
`lines`, `hidden`, `entry`, `position` (a `GridPos { row, column }`),
`line_of`, `cell_at`, `step`.

`Rollup` counts failing (failure, timed out, action required), running
(queued, in progress), passing, and other; `RollupState` ranks failing over
running over passing.

Selection is a `CellRef { repo, number, column }`, an identity, so a refresh
that reorders rows cannot move it. `CiGrid::step(from, CellMove)` moves
between cells: left and right within the row, up and down across rows — and
across repositories — in the same column, never wrapping, and entering at
the first cell when nothing (or a cell no longer in the grid) is selected.

The view (`CiView::rebuild`) rebuilds on every store change and splices the
list only when the lines changed, so a poll that only changed cell contents
keeps the scroll position.

## Time

`Timing::of(entry, now)` (`crates/rostrum-core/src/ci/time.rs`) takes `now`
explicitly, so it is tested against a fixed clock and the cell ticks simply by
being drawn again (the view redraws once a second while visible):

| State | Cell | Hover |
|---|---|---|
| queued, start known | `queued 2m 00s` | `queued for 2m 00s` |
| in progress | `3m 12s` | `running for 3m 12s` |
| finished | `finished 14m ago` | `took 4m 03s` |

`format_duration` shows at most two units with the smaller zero-padded
(`45s`, `3m 12s`, `1h 04m`, `2d 03h`); `format_ago` is coarser (`just now`,
`14m ago`, `3h ago`, `2d ago`). Clock skew clamps to zero.

## Logs

`GitHubClient::job_log` fetches `GET /repos/{o}/{r}/actions/jobs/{id}/logs`,
which redirects to short-lived storage on another host; reqwest follows it and
drops the token on the cross-host hop.

`parse_log(raw, LineLimit)` (`crates/rostrum-core/src/ci/log.rs`):

- strips the BOM, each line's timestamp prefix, and ANSI escape sequences;
- classifies lines by workflow command (`##[error]`, `##[warning]`,
  `##[notice]`, `##[debug]`, `[command]`, `##[group]`), removing the marker,
  and drops `##[endgroup]` lines while keeping the original line numbers;
- records groups (header to end) as collapsible sections;
- splits steps: a step's output follows its `##[group]Run …` header group, so
  a step runs from one top-level `Run …`/`Post …` header to the next, and
  everything before the first is "Set up job";
- finds the first error and the step holding it (`failing_step`).

`LineLimit::Last(DEFAULT_LOG_LINES)` (20 000) keeps the tail, where a failure
is, re-indexing groups and steps and recording `dropped`; the viewer offers
"Load full log", which re-parses the kept raw text with `LineLimit::Full`.

The viewer (`crates/rostrum/src/ci/log_view.rs`) draws `ParsedLog::visible`
lines with a `uniform_list`: groups start collapsed except one holding an
error (`default_collapsed`), the failing step is tinted and named in a
"failed step" chip, the first error is scrolled to, and search
(`ParsedLog::search`, case-insensitive) expands whatever hides a match
(`groups_hiding`) and steps through matches with Enter or Next.

For an app's check run, the pane shows `GitHubClient::check_output` — the
run's output title, summary and text as markdown, and its annotations
(`GET /check-runs/{id}` and `/annotations`) — with an "Open on <app>" link to
`detailsUrl`. A legacy status shows its link only.

## Re-runs

`rerun_targets(pr_checks, entry)` (`crates/rostrum-core/src/ci/rerun.rs`)
decides what a check offers:

| Check | Offers |
|---|---|
| Actions job, every job of its workflow run finished | `Job`, `FailedJobs` (if any job of the run failed or was cancelled), `AllJobs` |
| Actions job, any job of its run still queued or running | nothing: `NotRerunnable::StillRunning` |
| Another app's finished check with a suite | `Suite` (re-request) |
| Another app's running check | `StillRunning` |
| Another app's check without a suite | `NoSuite` |
| Legacy status | `LegacyStatus` |

`rerun_call(repo, target)` (`crates/rostrum-github/src/ci/rest.rs`) builds
the request: `POST /actions/jobs/{id}/rerun`, `POST
/actions/runs/{id}/rerun-failed-jobs`, `POST /actions/runs/{id}/rerun`, or
`POST /check-suites/{id}/rerequest`, with no body.

The flow (`CiView::ask_retry`, `CiView::rerun`): `r` shows a confirmation strip
naming what restarts (`RerunTarget::confirm_prompt`) with a button per offered
re-run, the first one primary (Enter picks it, Escape cancels). Confirming
calls `Store::requeue`, which applies `mark_requeued` — the covered cells flip
to queued with their times cleared — then sends the request. On success the
repository's checks are re-fetched after 4 s, long enough for GitHub to create
the new attempt; on failure, at once, which puts the old result back.

`classify_rerun` types the refusals as `RerunError`:

- `NoPermission` — a 403 about the token (not a collaborator with write
  access, or missing the `workflow`/`checks` permission);
- `NotRerunnable` — a 403 whose message is about the run (most often created
  over a month ago), or a 409/422;
- `NotFound` — the run or job is gone;
- `Api` — anything else, including a rate-limited 403, classified as usual.

## Desktop

- **Opening:** the **CI** button in the feed's filter bar, or `shift-c` in
  the feed (plain `c` collapses a repository). The grid takes the whole
  window below the title bar; `escape`, `shift-c` or "← Feed" returns.
- **Layout:** toolbar; the re-run confirmation or last outcome; one line per
  repository whose checks failed to load; the header row; the rows. With no
  rows, a message instead: "Loading checks…", "Nothing failing or running",
  or "No open pull requests outside drafts".
- **Scrolling:** the wheel scrolls rows; a horizontal wheel, a trackpad, or
  shift with the wheel scrolls the check columns, with the row headers and
  header row staying aligned.
- **Keys** (key context `CiGrid`): `h`/`j`/`k`/`l` or the arrows move between
  cells, `enter` opens the log (or confirms a pending re-run), `r` re-runs,
  `f` toggles failing-or-running only, `escape` closes the confirmation, then
  the log, then the view.
- **Mouse:** click selects a cell; double-click opens its log.
- **Log pane:** 640 px on the right while open.

## Invariants and constraints

- **One cell per column per pull request**, the latest attempt
  (`PrChecks::latest`); columns are unique across the grid, and every row has
  exactly one cell per column (`CiGrid` is only built by `build_grid`).
- **Column groups partition the columns** in order, each non-empty.
- **Columns never shift with the grid filter**; they are the union before it.
  They do follow the feed's search and authors, which decide the rows.
- **Drafts are never rows**, whatever the feed's draft toggle.
- **A row names its repository**: `(repo, number)` identifies it; the same
  number in two repositories is two rows.
- **Header and row headers stay aligned**: the header row and every row
  draw their check columns with the same horizontal offset.
- **Selection is by identity**, never by line index.
- **Time is a function of `now`.** Nothing in `rostrum-core::ci` reads the
  clock.
- **No re-run is offered on a run still going**, and none on a legacy status.
- **Optimistic flips are reconciled by the next fetch**, which replaces a
  repository's checks wholesale.
- **The faster poll exists only while the grid is visible** and touches only
  repositories with something running.

## For Android

The pure API the phone reuses, all in `rostrum_core::ci` unless noted:

- Model: `CheckStatus` (`from_check_run`, `from_status_context`, `is_running`,
  `is_failing`, `label`), `CheckKey`, `CheckSource`, `CheckEntry`
  (`producer`, `run_id`), `PrChecks` (`latest`, `rollup`), `Rollup`
  (`of`, `state`, `describe`), `RollupState`, `CheckOutput`, `Annotation`
  (`location`), `AnnotationLevel`.
- Grid: `CiChecks` (`begin`, `loaded`, `failed`, `forget`, `repo`, `pr`,
  `pr_mut`, `any_running`), `RepoChecks`, `build_grid`, `GridFilter`,
  `CiGrid` (`columns`, `column_groups`, `rows`, `row`, `repos`, `lines`,
  `hidden`, `entry`, `position`, `line_of`, `cell_at`, `step`),
  `ColumnGroup`, `GridRow` (with `repo`), `GridRepo`, `GridLine`, `GridPos`,
  `CellRef`, `CellMove`.
- Time: `Timing` (`of`, `label`, `duration_label`, `ticks`),
  `format_duration`, `format_ago`.
- Logs: `parse_log`, `LineLimit`, `DEFAULT_LOG_LINES`, `ParsedLog`
  (`default_collapsed`, `visible`, `group_at`, `search`, `groups_hiding`,
  `in_failing_step`), `LogLine`, `LineKind`, `LogGroup`, `LogStep`,
  `strip_ansi`.
- Re-runs: `rerun_targets`, `RerunTarget` (`label`, `confirm_prompt`),
  `NotRerunnable`, `mark_requeued`.
- `rostrum_github`: `GitHubClient::{ci_checks, job_log, check_output,
  rerun}`, `ci::RepoCiChecks`, `ci::RerunError`, `ci::rerun_call`,
  `ci::classify_rerun`, `ci::job_log_path`, `ci::CI_CHECKS`,
  `ci::CONTEXTS_PER_COMMIT`.

The Android app draws the matrix as the desktop does (see
`docs/features/android_ci_config.md`): the FFI's `CiGrid { columns, groups,
rows, repos, lines, hidden, ticks, anyRunning }` maps one to one to the
Kotlin model; one header (workflow band over job names) sits above the
list and shares one horizontal scroll with every row; rows are labelled
`owner/name #N`, a repository's first line is ruled off, stack lines are
kept, repositories whose checks failed to load are listed in a strip from
`CiGrid.repos`, and an empty grid says "Loading checks…", "Nothing failing
or running" or "No open pull requests outside drafts". A cell is opened by
its row and column index; re-run targets are still asked for by
repository, number and `CiCheckKey`. Opened from a repository, the phone
narrows the matrix to that repository's rows and the columns they ran.

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-core/src/ci/model.rs` | Statuses, keys, sources, entries, latest-attempt, rollups, check output | `CheckStatus`, `CheckKey`, `CheckSource`, `CheckEntry`, `PrChecks`, `Rollup`, `RollupState`, `CheckOutput`, `Annotation` |
| `crates/rostrum-core/src/ci/grid/mod.rs` | The matrix's types, lookup and cell movement | `CiGrid`, `GridRow`, `ColumnGroup`, `GridRepo`, `GridLine`, `GridPos`, `CellRef`, `CellMove`, `GridFilter` |
| `crates/rostrum-core/src/ci/grid/build.rs` | Building the matrix: rows filter, columns, groups, lines | `build_grid` |
| `crates/rostrum-core/src/ci/grid/store.rs` | Stored checks per repository | `CiChecks`, `RepoChecks` |
| `crates/rostrum-core/src/ci/grid/tests.rs` | Matrix tests | — |
| `crates/rostrum-core/src/ci/time.rs` | Elapsed and since-finished labels against `now` | `Timing`, `format_duration`, `format_ago` |
| `crates/rostrum-core/src/ci/log.rs` | Job log parsing | `parse_log`, `ParsedLog`, `LineLimit` |
| `crates/rostrum-core/src/ci/rerun.rs` | Re-run eligibility and the optimistic flip | `rerun_targets`, `RerunTarget`, `NotRerunnable`, `mark_requeued` |
| `crates/rostrum-github/src/ci/wire.rs` | The checks document and decoding | `CI_CHECKS`, `CiChecksData`, `ContextNode` |
| `crates/rostrum-github/src/ci/rest.rs` | Re-run requests and errors, log path, check output decoding | `rerun_call`, `classify_rerun`, `RerunError`, `job_log_path`, `check_output` |
| `crates/rostrum-github/src/ci/client.rs` | Client methods | `ci_checks`, `job_log`, `check_output`, `rerun`, `RepoCiChecks` |
| `crates/rostrum-github/src/ci/tests.rs` | Decoding, grid and log tests on captured data | — |
| `crates/rostrum-github/fixtures/ci/` | Captured rollups, a check run, annotations, a log excerpt | — |
| `crates/rostrum/src/sync/ci.rs` | Store: fetch, faster poll, requeue | `Store::{refresh_ci, set_ci_visible, requeue}`, `CI_WATCH_INTERVAL` |
| `crates/rostrum/src/ci/mod.rs` | The CI view entity, keys, selection, re-run flow | `CiView`, `CiEvent`, `bind_keys` |
| `crates/rostrum/src/ci/render.rs` | Toolbar, banner, failed repositories, empty state, wheel handling, layout | — |
| `crates/rostrum/src/ci/matrix.rs` | The header row, stack lines, row headers, cells, drawn at the horizontal offset | — |
| `crates/rostrum/src/ci/hscroll.rs` | The horizontal offset: clamp, wheel, reveal, columns in view | `HScroll` |
| `crates/rostrum-ffi/src/ci/` | The grid as Android records | `CiGrid`, `CiRow`, `CiColumnGroup`, `CiRepo`, `CiLine` |
| `crates/rostrum/src/ci/log_view.rs` | The log / output pane | `LogView` |

## Testing

- **Decoding** every context of three captured repositories: Actions jobs
  with run and workflow, running jobs, third-party app checks, legacy
  statuses; repeated check names reduced to one cell.
- **Matrix**: one column union across repositories and its order, workflow
  groups, "not run" gaps, rows across repositories in feed order, drafts
  excluded (also from columns, also inside a stack), the feed's search and
  authors applied, the attention filter without column shifts, collapse
  ignored, unfetched rows, per-repository load summaries, stack lines,
  cell movement across repositories, resolving cells and lines,
  any-running, load states; the whole pipeline on captured data; and the
  FFI grid end to end, drafts included.
- **Horizontal scroll**: clamping, wheel deltas, a shrunk grid, revealing a
  column, and the columns in view.
- **Time** with a fixed clock: duration and ago formats, running, finished,
  queued, legacy and unknown timings.
- **Logs**: timestamps, BOM, markers and ANSI removed; classification; groups;
  steps and the failing step; default collapse; folding; search; tail
  truncation; unclosed groups; and the captured cpython job excerpt.
- **Re-runs**: the request for each endpoint; eligibility for every source and
  state; the optimistic flip for each target; prompts; the error classes
  including a rate-limited 403.
