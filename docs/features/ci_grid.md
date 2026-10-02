# Feature: ci_grid

A grid of CI checks for every open pull request: status, how long each check
has been running or how long since it finished, its log, and re-running it.

## Scope

- A PRs × checks matrix over the whole window, reached from the feed.
- Each cell: a status tile (queued, in progress, success, failure, cancelled,
  skipped, neutral, timed out, action required, or "not run"), the live
  elapsed time while running or the time since it finished, and the duration
  on hover.
- Row headers: the pull request (number, title, short head sha) and a rollup.
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
- Android. The rules are pure, in the shared crates, for it to reuse; see
  [For Android](#for-android).

## Layout decision: per-repository columns

Each repository is a section with its own header row of columns, the union
of the check names on its pull requests. Global columns would have been the
union across all repositories, and watched repositories rarely share check
names, so most cells of every row would be "not run" for columns belonging
to other repositories. Per-section columns keep every drawn column meaningful.
The whole grid scrolls horizontally as one piece, sized to the widest section,
so row headers and column headers stay aligned with their cells.

Columns are keyed by `CheckKey { workflow, name }` (an Actions job's workflow
plus job name; no workflow for apps and statuses), because two workflows
routinely both have a `build` job. They are ordered by workflow then name,
which puts a workflow's jobs side by side.

Rows follow the feed: its repository and item sort, its filter (search,
authors, drafts), and its stack grouping, with a "Stack · N PRs" line heading
each stack and a ┏ ┃ ┗ mark on members. Collapsing a repository in the feed
does not hide it in the grid: the grid is where its checks are read.

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
(`crates/rostrum-core/src/ci/grid.rs`):

1. Flatten the pull request feed with collapse cleared, which gives the
   feed's order and stack slots.
2. Per repository section: the columns are the union of `PrChecks::latest()`
   keys over its pull requests, computed before the grid filter so filtering
   never shifts columns.
3. Each row: one cell per column (`None` is "not run"), the rollup over the
   latest entries, the short sha, the stack place, whether checks were
   fetched at all, and whether they were truncated.
4. `GridFilter { needs_attention }` keeps rows whose rollup is failing or
   running, and drops sections it empties.
5. The result is flattened into `GridLine`s — `Header`, `Stack`, `Row`,
   `Notice`, `Spacer` — for one virtualized `list`.

`Rollup` counts failing (failure, timed out, action required), running
(queued, in progress), passing, and other; `RollupState` ranks failing over
running over passing.

Selection is a `CellRef { repo, number, column }`, an identity, so a refresh
that reorders rows cannot move it. `CiGrid::step(from, CellMove)` moves
between cells: left and right within the row, up and down across rows and
sections keeping the column by name where the next section has it and
clamping otherwise, never wrapping, and entering at the first cell when
nothing is selected.

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
- **Keys** (key context `CiGrid`): `h`/`j`/`k`/`l` or the arrows move between
  cells, `enter` opens the log (or confirms a pending re-run), `r` re-runs,
  `f` toggles failing-or-running only, `escape` closes the confirmation, then
  the log, then the view.
- **Mouse:** click selects a cell; double-click opens its log.
- **Log pane:** 640 px on the right while open.

## Invariants and constraints

- **One cell per column per pull request**, the latest attempt
  (`PrChecks::latest`); columns are unique within a section.
- **Columns never shift with the grid filter**; they are the union before it.
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
  `CiGrid` (`sections`, `lines`, `section`, `entry`, `position`, `line_of`,
  `step`, `max_columns`), `GridSection`, `GridRow`, `GridLine`, `CellRef`,
  `CellMove`.
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

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-core/src/ci/model.rs` | Statuses, keys, sources, entries, latest-attempt, rollups, check output | `CheckStatus`, `CheckKey`, `CheckSource`, `CheckEntry`, `PrChecks`, `Rollup`, `RollupState`, `CheckOutput`, `Annotation` |
| `crates/rostrum-core/src/ci/grid.rs` | Stored checks, the matrix, lines, filter, cell movement | `CiChecks`, `build_grid`, `CiGrid`, `GridLine`, `CellRef`, `CellMove`, `GridFilter` |
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
| `crates/rostrum/src/ci/render.rs` | Toolbar, banner, header, rows, cells | — |
| `crates/rostrum/src/ci/log_view.rs` | The log / output pane | `LogView` |

## Testing

- **Decoding** every context of three captured repositories: Actions jobs
  with run and workflow, running jobs, third-party app checks, legacy
  statuses; repeated check names reduced to one cell.
- **Matrix**: column union and order, "not run" gaps, lines, the attention
  filter, feed order without collapse, unfetched rows, notices, cell
  movement across sections, resolving cells, any-running, load states; and
  the whole pipeline on captured data.
- **Time** with a fixed clock: duration and ago formats, running, finished,
  queued, legacy and unknown timings.
- **Logs**: timestamps, BOM, markers and ANSI removed; classification; groups;
  steps and the failing step; default collapse; folding; search; tail
  truncation; unclosed groups; and the captured cpython job excerpt.
- **Re-runs**: the request for each endpoint; eligibility for every source and
  state; the optimistic flip for each target; prompts; the error classes
  including a rate-limited 403.
