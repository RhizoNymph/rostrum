# Feature: poll_budget

Keeping the desktop's and the phone's background polling inside GitHub's
rate limit, which they share with the `gh` CLI (one token, one 5,000
points-per-hour GraphQL budget and one 5,000 requests-per-hour REST budget).
The phone runs the same rules from the same crates; see
[Android](#android).

## Scope

- **Batching.** Open pull requests, open issues, CI checks and divergence
  for several repositories in one GraphQL document each
  (`r0: repository(owner: $o0, name: $n0) { … }`), chunked by a cost model
  that picks the cheapest chunk size per kind. A failure on one alias is
  that repository's error; the rest apply.
- **CI skip rules.** A repository's checks are fetched on a poll only when
  something could have changed; otherwise the held checks are reused.
- **Issue cadence.** Issues are refreshed every `issue_refresh_secs`
  (default 300) instead of every poll.
- **Budget tracking.** Every response's `x-ratelimit-*` headers, every
  rate-limit refusal and every request's cost, per feature, in one
  `BudgetState` the client keeps and the store reads.
- **Back-off.** The poll interval stretches as the GraphQL budget runs down,
  paced so the remainder lasts until the reset; polling stops entirely while
  GitHub refuses (primary or secondary limit).
- **Readout.** A status-bar chip: points left of the limit and the reset
  time, coloured by level, or "paused until HH:MM".
- **Spend log.** Every five minutes, an info line with the points each
  feature spent.
- **Android.** The same planner, pacing, pauses, batches and spend log in
  `rostrum-ffi`; the budget per profile to Kotlin, a chip and explanation
  sheet on the feed, and the WorkManager check skipping a paused profile.

## Non-scope

- Persisting a pause across processes on the phone. The budget lives in
  the client's memory; a WorkManager run in a fresh process learns of a
  pause from its first refusal.
- Knowing for certain where a phone profile's token came from. A desktop
  profile's token is taken to be the desktop's (handed over when paired, the
  usual case); a token pasted into a desktop profile is described the same
  way.
- Batching the Closed tab's lists (fetched only while that tab shows) and
  the stacks REST reads (REST core budget, separate from GraphQL).
- Skipping divergence when nothing moved. It now costs one point per poll
  for all repositories together; skipping would need each pull request's
  base tip in the feed query.
- Pausing on-demand reads (opening a pull request, logs, mutations). The
  client still records their budget and refusals, but they are the user's
  explicit actions and are sent.

## Cost, before and after

GitHub's documented calculation: add up the requests each connection needs
(a connection under `n` parents is `n` requests), divide by 100, round to
the nearest point, at least 1 — **per document**. Verified with
`rateLimit(dryRun: true) { cost nodeCount }` at `first: 25`:

| kind | per repository | 1 repo | 5 | 10 | 20 |
|---|---|---|---|---|---|
| open pull requests | 1.26 points, 825 nodes | 1 | 6 | 13 | 25 |
| issues | 0.51, 525 nodes | 1 | 3 | 5 | 10 |
| CI checks | 0.51, 2,550 nodes | 1 | 3 | 5 | 10 |
| pull requests + checks in one selection | 1.51, 3,325 nodes | 2 | 8 | 15 | 30 |
| divergence (`Ref.compare`, no connections) | 0 | 1 | 1 | 1 | 1 (40 repos: 1) |

Before, per poll: `R` pull request queries + `R` issue queries + `R` CI
queries + `R` divergence queries, one point each — `4R` points (more with
the Closed tab and merge probes). At 60 s that is `4R × 60` an hour: 1,200
for five repositories, the ~1,100/h measured.

After, per hour at refresh interval `T` seconds, `R` repositories, 25 per
repository:

- pull requests: `R × 3600/T` (one point each; see below why they are not
  batched at this page size),
- divergence: `⌈R/20⌉ × 3600/T`,
- issues: `⌈0.51·R⌉ × 3600/issue_refresh_secs`,
- CI: `⌈0.51·R⌉ × 6` for the ten-minute safety net, plus one batch for each
  poll where a head moved, a pull request opened or a check was running.

Five repositories at 60 s: 300 + 60 + 36 + 18 ≈ **414/h** plus CI activity,
against 1,200. Thirty repositories: 1,800 + 120 + 192 + 96 ≈ 2,200/h, where
the old shape needed 7,200 and could not run at all.

### Decisions the arithmetic made

- **Pull requests go one repository at a time at 25 per repository.** A lone
  repository's 1.26 rounds down to 1; any batch pays the full 1.26 each
  (two repositories: 2.52 → 3). `chunk_size` evaluates every size up to 20
  and keeps the cheapest, largest on a tie, so the batched path is used —
  it simply chooses chunks of one here. At 50 per repository (2.51 each) it
  chooses pairs.
- **Issues and checks go all together** (five repositories: 3 points in one
  request instead of 5 in five).
- **Pull requests and checks stay separate documents.** Combined they cost
  1.51 per repository on *every* poll; separate, the pull requests cost 1.26
  (rounded to 1) every poll and the checks 0.51 only on polls where the skip
  rules say they could have changed. The combined selection is also 4×
  the nodes.
- **Chunk bounds.** At most 20 repositories (`MAX_CHUNK`) and 50,000 nodes
  (`NODE_BUDGET`, a tenth of GitHub's 500,000 cap) per document, which keeps
  responses a few megabytes at most and limits what one failed request
  takes with it. CI at 100 pull requests per repository (10,200 nodes each)
  is the kind the node budget binds: four repositories per document.

## Data and control flow

```
start_polling ─► loop { delay = next_poll_delay(); timer(delay); poll_tick(Timer) }
                       │                                   │
   pause in force? ────┘ wait until it lifts               ▼
   else pace(base, graphql budget, poll_cost())     plan_poll(repos, PollContext)
                                                     ├─ Paused(pause) → log, nothing sent
                                                     └─ Run(PollPlan { feed, issues, closed })
                                                          │
         ┌────────────────────────────────────────────────┼──────────────────────┐
         ▼                                                ▼                      ▼
 refresh_feed(feed, Poll)                     refresh_issues_batch(issues)   refresh_closed(each)
   chunk_size(OpenPulls) → open_pull_requests_batch per chunk
   apply_feed_batch: viewer, per-repo apply_pulls / feed_failed, landed += ok
   last batch landed ─► feed_settled:
        fetch_divergences(landed)  → divergences_batch (one doc per ≤20 repos)
        fetch_stacks(each landed)  → REST
        refresh_ci_planned(landed, trigger) → plan_ci → ci_checks_batch
        schedule_merge_probe       → timer → refresh_feed(computing, Probe)
```

- **The Refresh action** is `poll_tick(Explicit)`: every issue list, and the
  cycle is marked explicit so `plan_ci` fetches every landed repository's
  checks. It too sends nothing while paused.
- **Adding a repository** (UI or `config.json`) refreshes it through the
  same batch path; its follow-ups run when its batch lands.
- **Showing the Issues tab** (`set_tab`) refreshes every repository whose
  issues are older than the cadence.
- **The CI view** opening runs `refresh_ci_planned(all, Timer)`; while open,
  every 15 s `running_repos` (repositories with anything queued or running)
  go out as one `ci_checks_batch`. A re-run reconciles through
  `refresh_ci(repo)`, a batch of one.
- **Merge probes** are one timer for the whole cycle: each probe re-queries
  every repository still computing in one feed batch (origin `Probe`, tallied
  as `probes`), backing off 2/4/8 s, three per cycle. A probe's landing does
  not re-run divergence, stacks or checks.
- **Every fetch path** (`refresh_feed`, `refresh_issues_batch`,
  `refresh_ci_batch`, `fetch_divergences`, `fetch_stacks`, `refresh_closed`)
  asks `Store::paused` first, so the CI watch, probes and follow-ups stop
  with the poll.

### Budget tracking

`GitHubClient` holds a `BudgetTracker` (an `Arc<Mutex<BudgetState>>` shared
by every clone, locked only to copy or fold plain data) and a `Feature` tag.
Every request goes through `GitHubClient::transmit`, which records:

1. the `x-ratelimit-resource`/`-limit`/`-remaining`/`-reset` headers as a
   `RateBudget` for `graphql` or `core` (`Budgets::observe` keeps the lower
   remainder within one window, replaces on a later window, ignores an
   earlier one — responses land out of order);
2. a refusal as a `RateLimitHit`: 403/429 with `retry-after` or GitHub's
   "secondary rate limit" wording is `Secondary` (retry-after, default 60 s);
   with `x-ratelimit-remaining: 0` it is `Primary` (until
   `x-ratelimit-reset`). A GraphQL 200 whose errors include `RATE_LIMITED` is
   `Primary` too, and becomes `GitHubError::RateLimited`;
3. the spend: a GraphQL response's `rateLimit.cost` (one point when the
   document did not select it) or one REST request, under the clone's
   `Feature`.

The store tags its clients (`client_for(Feature::Ci)` …); `Store::client()`,
which the views use, is untagged and counts as `detail`.

A hit becomes a `Pause` (`Pause::after`): until the reset (or now + 60 s
when the reset is already past — GitHub just refused, so the clocks
disagree), or now + retry-after, plus 5–30 s of jitter so the phone and the
desktop do not resume in the same second. A pause in force is only ever
lengthened.

### Back-off policy (`rostrum_core::budget::pace`)

1. Unknown budget, a window that has reset, or ≥ 20% left: the configured
   interval.
2. Hold back a reserve of 2% of the limit (100 points) for interactive use.
   If what is left above it cannot pay for one more poll: wait for the reset.
3. Otherwise the longer of a floor (2× the interval below 20%, 4× below 10%)
   and the pace that spreads the usable remainder over the time to reset at
   the estimated cost per poll (`poll_cost_estimate`).
4. Never longer than the time to reset (the budget refills then), unless the
   configured interval is.

Pacing to the reset, not multipliers alone, is what keeps the budget
lasting when `gh` and the phone spend it too: the remainder it divides is
GitHub's, whoever spent the rest.

### CI skip rules (`rostrum_core::poll::ci_due`)

For each repository whose pull requests just landed, against
`RepoChecks` (the last fetch's checks and load state), drafts left out on
both sides (the CI view never shows them):

| condition | decision |
|---|---|
| explicit refresh | fetch (`Explicit`) |
| never fetched (`Idle` / no entry) | fetch (`NeverFetched`) |
| first fetch in flight (`Loading`) | leave (`InFlight`) |
| last fetch failed | fetch (`LastFailed`) |
| a pull request the held checks do not cover | fetch (`PrAdded`) |
| a pull request whose head SHA differs from its checks' | fetch (`HeadMoved`) |
| any held check queued or in progress | fetch (`Running`) |
| last fetch ≥ 10 min ago (`CI_SAFETY_INTERVAL`) | fetch (`SafetyNet`) |
| otherwise | reuse |

A closed pull request changes nothing: the grid is built from the feed's
pull requests, so its stale checks are never shown. The safety net exists
because another app can add a check to an unchanged head (a late status, a
re-run from the web); ten minutes bounds that staleness at six batches an
hour (≈18 points for five repositories). A re-run from rostrum flips its
cells to queued, which the running rule then re-fetches.

### Issue cadence (`rostrum_core::poll::issues_due`)

`Idle` or `Failed`: due. `Loading`: not due. `Loaded { at }`: due once
`now − at ≥ issue_refresh_secs`, or at once for an explicit refresh.
`issue_refresh_secs` is clamped to 10 s – 1 day; it is a machine setting
(not in `SHARED_KEYS`), merged by `merge3` like `refresh_secs`.

### Readout and log

`budget_chip` (pure, `crates/rostrum/src/budget_chip.rs`) turns the
`BudgetState`, the last stretched `Pace` and the time into the chip:
`API 4123/5000 · resets 14:05` (quiet ≥ 20%, warning below, danger below
10% or at 0), `API 5000/5000` once the window has reset, or
`paused until 14:32` (danger). The tooltip says the budget is shared with
`gh` and the phone, lists GraphQL and REST remainders, and says how polling
slows (with the current interval when stretched). Nothing shows before the
first response.

Every `SPEND_LOG_INTERVAL` (5 min), when anything was spent:

```
INFO rostrum::budget: GitHub budget spent minutes=5 feed=25 issues=3 ci=3 closed=0
     divergence=5 probes=0 detail=0 graphql_points=36 graphql_requests=36
     stacks_rest=25 rest_requests=25 graphql_remaining=4612 core_remaining=4950
```

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-core/src/budget/mod.rs` | `BudgetState`: budgets, pause, tally | `BudgetState` |
| `crates/rostrum-core/src/budget/rate.rs` | A budget as reported, its level, merging observations | `RateResource`, `RateBudget`, `BudgetLevel`, `Budgets`, `LOW_FRACTION`, `CRITICAL_FRACTION` |
| `crates/rostrum-core/src/budget/pause.rs` | Refusals and the pause they call for | `RateLimitHit`, `Pause`, `PauseCause`, `Jitter`, `SECONDARY_DEFAULT_RETRY` |
| `crates/rostrum-core/src/budget/pace.rs` | The back-off policy | `pace`, `Pace`, `PaceReason`, `reserve`, `RESERVE_FRACTION` |
| `crates/rostrum-core/src/budget/spend.rs` | Points and requests per feature | `Feature`, `SpendTally` |
| `crates/rostrum-core/src/poll/mod.rs` | Cadences and triggers | `Trigger`, `Cadence`, `DEFAULT_ISSUE_INTERVAL`, `CI_SAFETY_INTERVAL` |
| `crates/rostrum-core/src/poll/plan.rs` | The poll planner | `plan_poll`, `PollContext`, `PollDecision`, `PollPlan` |
| `crates/rostrum-core/src/poll/issues.rs` | Issue cadence | `issues_due` |
| `crates/rostrum-core/src/poll/ci.rs` | CI skip rules, the CI plan, the watch's repositories | `ci_due`, `CiDue`, `CiReason`, `plan_ci`, `running_repos` |
| `crates/rostrum-github/src/batch/mod.rs` | Batch results | `RepoBatch`, `FetchedPulls` |
| `crates/rostrum-github/src/batch/document.rs` | Aliased documents and their variables | `build_repo_batch`, `repo_batch_variables`, `repo_alias` |
| `crates/rostrum-github/src/batch/decode.rs` | Splitting an answer per repository, per-alias errors | `BatchData`, `split_batch` |
| `crates/rostrum-github/src/batch/cost.rs` | The cost model and chunking | `BatchKind`, `chunk_cost`, `chunk_size`, `chunks`, `PollShape`, `poll_cost_estimate`, `MAX_CHUNK`, `NODE_BUDGET` |
| `crates/rostrum-github/src/batch/divergence.rs` | Divergence across repositories | `RepoPairs`, `build_divergence_document`, `excused_pair_error` |
| `crates/rostrum-github/src/batch/client.rs` | The batched client methods | `GitHubClient::{open_pull_requests_batch, open_issues_batch, ci_checks_batch, divergences_batch}` |
| `crates/rostrum-github/src/client/budget.rs` | The shared tracker, header and refusal parsing | `BudgetTracker` |
| `crates/rostrum-github/src/client.rs` | `transmit` records every response; `for_feature`, `budget`, `take_spend` | `GitHubClient` |
| `crates/rostrum-github/fixtures/batch/` | Captured batched answers (pull requests, issues, checks over zed, rust and a missing repository) | |
| `crates/rostrum-config/src/lib.rs` | `issue_refresh_secs`, `issue_refresh_interval` | `Config` |
| `crates/rostrum/src/sync/poll.rs` | Poll loop, tick, pacing, pause check, spend log | `Store::{refresh_all, budget, pace}`, `SPEND_LOG_INTERVAL` |
| `crates/rostrum/src/sync/feed.rs` | Feed batches and their follow-ups, merge probes, divergence | `Store::refresh_repo`, `FeedOrigin`, `FeedCycle` |
| `crates/rostrum/src/sync/issues.rs` | Issue batches, stale-tab refresh | `Store::refresh_issues` |
| `crates/rostrum/src/sync/ci.rs` | Planned and watched checks batches | `Store::{refresh_ci, set_ci_visible}` |
| `crates/rostrum/src/sync/inflight.rs` | Per-repository overlap guard for batches | `InFlight`, `BatchId` |
| `crates/rostrum/src/budget_chip.rs` | The status bar chip's wording | `budget_chip`, `BudgetChip`, `Tone` |

## Invariants

- A repository is in at most one in-flight batch per kind (`InFlight`); a
  repository already in flight is left out of a new batch.
- One alias's failure never fails another alias's repository; an error
  outside every alias fails the whole document (as a one-repository
  document would have).
- Follow-ups (divergence, stacks, CI planning, probes) run once per poll,
  after the last feed batch lands, over the repositories that landed.
- An older divergence batch never writes over a newer one for the same
  repository (`divergence_latest`).
- Nothing in the background is sent while a pause is in force; a pause is
  only lengthened, never shortened, while it holds.
- A rate-limit refusal does not mark a repository that has data as failed:
  the data stays and the chip says why nothing is moving.
- `RateBudget` never has a zero limit or a remainder above it; observations
  from an earlier window never overwrite a later one.
- The token never reaches a log; the tracker logs only the hit and status.

## Android

`rostrum-ffi` (`docs/features/android_core.md`) adopts the same rules; each
profile's `RostrumCore` has its own session and so its own token's budget.

```
Kotlin FeedViewModel.autoRefreshWhileVisible:
  loop { delay(pollSchedule().delaySecs); pollFeed() }
                    │                          │
  budget::schedule ─┘                          ▼
   pause in force → its remaining time   begin_refresh(Scope::Poll(Timer))
   else pace(settings interval,            plan_poll(feed.repos, PollContext)
        graphql, poll_cost_estimate)        ├─ Paused → snapshot as it stands
                                            └─ Run → land_pulls ║ land_issues  (batches, ≤4 in flight)
                                                       last pull chunk landed:
                                                         land_divergences ║ land_stacks
                                                         schedule_probe (one per cycle)
                                                         log_spend_if_due
```

- **Read the budget.** `CoreState::budget_state` is `client.budget()`;
  clients are tagged per feature with `for_feature` (`Feed`, `Issues`,
  `Divergence`, `Stacks`, `Probes`, `Ci`; on-demand calls stay `Detail`).
- **Gate on the pause.** `begin_refresh` (feed, issues, the notification
  check), `land_divergences`, `land_stacks`, the probe, `refresh_issues` and
  `plan_checks` (the CI grid, and the re-fetches after a re-run) all ask for
  the pause first and send nothing while one holds. Explicit requests
  (`refreshFeed`, `refreshRepo`, `refreshCi`, `refreshCiRepo`,
  `checkNotifications`) fail with `GitHubRateLimited { resets_at: until }`;
  the timer's `pollFeed` and the grid's `openCi`/`watchCi` return quietly.
- **Pace.** `pollSchedule()` (and the snapshot's `budget.schedule`) is the
  pause's remaining time, or `pace(settings interval, graphql budget,
  poll_cost_estimate(..))`; Kotlin waits it between `pollFeed` calls instead
  of the fixed interval. A change of pace is logged.
- **Plan.** `plan_poll` decides the feed and issues for `pollFeed`
  (`Trigger::Timer`: issues once older than `issue_refresh_secs`, a phone
  setting, default `DEFAULT_ISSUE_INTERVAL`, clamped 10 s – 1 day) and
  `refreshFeed` (`Explicit`: everything). The grid uses `plan_ci` when it
  opens (`Timer`) and on pull to refresh (`Explicit`), and `running_repos`
  for its 15 s watch.
- **Batch.** Pull requests, issues, checks and divergence go out through the
  `*_batch` methods, chunked by `chunk_size`; one alias failing is that
  repository's error; a whole document failing is each of its
  repositories'. Merge probes are one per cycle over every repository still
  computing. The notification check reads pull requests only (no
  divergence).
- **Readout.** `ApiBudget` — GraphQL and REST `RateWindow`s (remaining as
  of now, limit, reset or none once reset, `ApiBudgetLevel` by
  `BudgetLevel`) and the `PollSchedule` (`Normal`, `Stretched(level)`,
  `UntilReset`, `Paused(until, reason)`) — from `apiBudget()` and on every
  `FeedSnapshot`. The feed shows a chip right of the filter chips
  (`ui/feed/BudgetChip.kt`): `API 4123/5000 · resets 14:05` (neutral,
  warning below 20%, danger below 10% or spent), `API 5000/5000` once
  reset, `paused until 14:32` (danger). Tapping it opens a sheet saying the
  budget is shared with the desktop and `gh` when the profile's token came
  from a desktop (`BudgetSharing.of(ProfileKind)`), each window, and how
  refreshing adapts.
- **Notifications.** `NotificationCheck` asks each profile's
  `apiBudget().pausedUntil` first and records `ProfileCheck.Paused(until)`
  without a request; the other profiles (other tokens) go ahead. The core
  refuses as well, should it be asked.
- **Spend log.** The same structured line (target `rostrum::budget`), at
  most every five minutes, checked at the end of each refresh and CI fetch.

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-ffi/src/budget/mod.rs` | Budget, pause checks, poll cost and schedule, exports | `RostrumCore::{api_budget, poll_schedule}` |
| `crates/rostrum-ffi/src/budget/types.rs` | Records and pure conversions | `ApiBudget`, `RateWindow`, `PollSchedule`, `PollPace`, `schedule`, `api_budget` |
| `crates/rostrum-ffi/src/budget/spend.rs` | The spend log | `SpendLog` |
| `crates/rostrum-ffi/src/feed/refresh.rs` | `plan_poll`, the pause, running a refresh | `Scope`, `Refreshed` |
| `crates/rostrum-ffi/src/feed/{batch,land,apply,probe}.rs` | Chunked batches, landing, applying, the probe cycle | `land_pulls`, `ProbeCycle`, … |
| `crates/rostrum-ffi/src/ci/fetch.rs` | `plan_ci`/`running_repos`, the pause, checks batches | `CiFetch`, `land_checks` |
| `crates/rostrum-ffi/src/settings.rs` | `issue_refresh_secs` | `set_issue_refresh_interval` |
| `android/.../data/BudgetApi.kt`, `data/model/Budget.kt`, `data/ffi/BudgetMappings.kt` | The budget in Kotlin | `BudgetApi`, `ApiBudget`, … |
| `android/.../ui/feed/BudgetChip.kt`, `BudgetSheet.kt` | Chip wording, colours, sheet | `budgetChip`, `budgetExplanation`, `BudgetSharing` |
| `android/.../notifications/NotificationWorker.kt` | Skipping a paused profile | `ProfileCheck.Paused` |

## Testing

- `rostrum-core`: `budget::{rate, pause, pace, spend}` (thresholds,
  merging, pause lengthening, jitter bounds, every pacing branch) and
  `poll::{issues, ci, plan}` (cadence, every CI rule, drafts, the plan with
  and without a pause).
- `rostrum-github`: `batch::cost` (the model against GitHub's dry-run costs,
  chunk choice), `batch::tests` (documents and variables agree, captured
  answers decode per repository, `NOT_FOUND`/`FORBIDDEN`/nested/batch-level
  errors, divergence excusal, cost reading, `RATE_LIMITED`), and
  `client::budget` (headers, primary/secondary hits, sharing across clones).
- `rostrum-config`: default, clamp, not shared.
- `rostrum`: `budget_chip` wording and tones.
- `rostrum-ffi`: `budget::types` (windows, levels, every pace, pauses,
  rounding), `budget::spend`, `feed::batch` (chunk sizes), settings; and
  `tests/poll_budget.rs` against the GitHub stand-in (which now answers
  batched documents and can send rate-limit headers and secondary-limit
  refusals): batched documents per kind, the issue cadence, the readout and
  a stretched schedule, the CI skip rules (open, watch, explicit), one probe
  for every computing repository, and a pause stopping the timer, the
  notification check and the grid while explicit calls say until when.
- Android (JVM): `BudgetChipTest` (chip wording and colours, the sheet),
  `BudgetMappingsTest`, `FakeBudgetTest`, `FeedViewModelTest` (the paced
  timer, a pause, the sheet), `CiGridViewModelTest` (each fetch's trigger),
  `NotificationsTest` (a paused profile skipped), and the host smoke test.
