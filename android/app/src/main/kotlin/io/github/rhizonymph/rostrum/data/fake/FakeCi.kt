package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.CiApi
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiCheckOutput
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiNotRerunnable
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.CiRollup
import io.github.rhizonymph.rostrum.data.model.CiRollupState
import io.github.rhizonymph.rostrum.data.model.CiRow
import io.github.rhizonymph.rostrum.data.model.CiSection
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import java.time.Clock
import java.time.Duration
import java.time.Instant

/**
 * The CI grid over the fake's open pull requests and [SampleCi]'s checks.
 * Like the core, nothing is known until [refreshCi] fetches it; [ciGrid]
 * rebuilds the labels against the clock. A re-run flips the checks it covers
 * to queued; the next refreshes move them to running, then passing.
 */
internal class FakeCi(
    private val host: FakeHost,
    private val clock: Clock,
    private val started: Instant,
) : CiApi {
    private data class Spot(val repo: String, val number: Int, val column: Int)

    private val fetched = mutableMapOf<String, Instant>()

    /** Re-run checks and how many refreshes they have seen since. */
    private val requeued = mutableMapOf<Spot, Int>()

    /** Re-run checks that have finished: they passed. */
    private val passed = mutableSetOf<Spot>()

    /** How many re-runs were asked for, grids built and fetches made, for tests. */
    var reruns = 0
        private set
    var builds = 0
        private set
    var fetches = 0
        private set

    override suspend fun ciGrid(filter: CiGridFilter): Outcome<CiGrid> =
        host.call(FakeCall.CiGrid) { host.signedIn { Outcome.Ok(grid(filter)) } }

    override suspend fun refreshCi(filter: CiGridFilter): Outcome<CiGrid> = host.call(FakeCall.RefreshCi) {
        host.signedIn {
            fetch(host.watchedRepos)
            Outcome.Ok(grid(filter))
        }
    }

    override suspend fun refreshCiRepo(repo: String, filter: CiGridFilter): Outcome<CiGrid> =
        host.call(FakeCall.RefreshCiRepo) {
            host.signedIn {
                if (host.watchedRepos.none { it == repo }) {
                    Outcome.Err(BackendError.InvalidInput("$repo isn't watched"))
                } else {
                    fetch(listOf(repo))
                    Outcome.Ok(grid(filter))
                }
            }
        }

    override suspend fun jobLog(repo: String, jobId: Long, full: Boolean): Outcome<CiJobLog> =
        host.call(FakeCall.JobLog) {
            host.signedIn {
                val spot = spotOf(repo, jobId)
                    ?: return@signedIn Outcome.Err(BackendError.GitHubApi(404, "job $jobId not found"))
                val status = status(spot)
                Outcome.Ok(SampleCi.jobLog(failed = status == CiStatus.Failure, full = full))
            }
        }

    override suspend fun checkOutput(repo: String, checkRunId: Long): Outcome<CiCheckOutput> =
        host.call(FakeCall.CheckOutput) { host.signedIn { Outcome.Ok(SampleCi.checkOutput()) } }

    override suspend fun rerunTargets(repo: String, pr: Int, key: CiCheckKey): Outcome<CiRerunChoice> =
        host.call(FakeCall.RerunTargets) {
            host.signedIn {
                val column = SampleCi.columns(repo).indexOfFirst { it.column.key == key }
                val spot = Spot(repo, pr, column)
                val open = host.openPulls(repo).any { it.number == pr }
                val cell = if (column < 0 || !open || repo !in fetched) null else cell(spot)
                if (cell == null) return@signedIn Outcome.Err(BackendError.InvalidInput("that check did not run on #$pr"))
                Outcome.Ok(choice(spot, cell))
            }
        }

    override suspend fun rerun(repo: String, target: CiRerun): Outcome<Unit> = host.call(FakeCall.Rerun) {
        host.signedIn {
            val covered = covered(repo, target)
            if (covered.isEmpty()) return@signedIn Outcome.Err(BackendError.CiNotFound)
            covered.forEach {
                requeued[it] = 0
                passed -= it
            }
            reruns++
            Outcome.Ok(Unit)
        }
    }

    private fun fetch(repos: List<String>) {
        fetches++
        val now = clock.instant()
        repos.forEach { fetched[it] = now }
        requeued.replaceAll { spot, seen -> if (spot.repo in repos) seen + 1 else seen }
        val done = requeued.filterValues { it >= 2 }.keys
        passed += done
        requeued.keys.removeAll(done)
    }

    // --- the grid ----------------------------------------------------------------------

    private fun grid(filter: CiGridFilter): CiGrid {
        builds++
        val sections = host.watchedRepos.map { section(it, filter) }
        val lines = buildList {
            sections.forEachIndexed { index, section ->
                if (index > 0) add(CiLine.Spacer)
                add(CiLine.Header(index))
                if (section.rows.isEmpty()) add(CiLine.Notice(index))
                section.rows.indices.forEach { add(CiLine.Row(index, it)) }
            }
        }
        val cells = sections.flatMap { s -> s.rows.flatMap { it.cells.filterNotNull() } }
        return CiGrid(
            sections = sections,
            lines = lines,
            ticks = cells.any { it.ticks },
            anyRunning = cells.any { it.status == CiStatus.Queued || it.status == CiStatus.InProgress },
        )
    }

    private fun section(repo: String, filter: CiGridFilter): CiSection {
        val checks = SampleCi.columns(repo)
        val at = fetched[repo]
        val rows = host.openPulls(repo).sortedByDescending { it.number }.map { pull ->
            val cells = checks.indices.map { column -> if (at == null) null else cell(Spot(repo, pull.number, column)) }
            CiRow(
                number = pull.number,
                title = pull.title,
                headSha = "%07x".format((repo.hashCode().toLong() * 31 + pull.number) and 0xFFFFFFF),
                rollup = rollup(cells),
                cells = cells,
                stack = null,
                fetched = at != null,
                truncated = false,
            )
        }
        val shown = if (filter.needsAttention) {
            rows.filter { it.rollup.state == CiRollupState.Failing || it.rollup.state == CiRollupState.Running }
        } else {
            rows
        }
        return CiSection(
            repo = repo,
            columns = checks.map { it.column },
            rows = shown,
            load = at?.let { RepoLoad.Loaded(it) } ?: RepoLoad.Idle,
            hidden = rows.size - shown.size,
        )
    }

    private fun status(spot: Spot): CiStatus? = when {
        requeued[spot] == 0 -> CiStatus.Queued
        requeued[spot] == 1 -> CiStatus.InProgress
        spot in passed -> CiStatus.Success
        else -> SampleCi.seed(spot.repo, spot.number, spot.column)?.status
    }

    private fun cell(spot: Spot): CiCell? {
        val seed = SampleCi.seed(spot.repo, spot.number, spot.column) ?: return null
        val check = SampleCi.columns(spot.repo)[spot.column]
        val status = status(spot) ?: return null
        val now = clock.instant()
        val since = if (spot in requeued) fetched[spot.repo] ?: now else started.minus(seed.age)
        val elapsed = Duration.between(since, now).coerceAtLeast(Duration.ZERO)
        val running = status == CiStatus.InProgress || status == CiStatus.Queued
        val (timing, duration) = when (status) {
            CiStatus.Queued -> "queued ${clockLabel(elapsed)}" to null
            CiStatus.InProgress -> clockLabel(elapsed) to "running for ${clockLabel(elapsed)}"
            else -> "finished ${agoLabel(elapsed)} ago" to "took ${clockLabel(Duration.ofSeconds(150L + spot.number * 13L))}"
        }
        return CiCell(
            status = status,
            statusLabel = statusLabel(status),
            role = roleOf(status),
            timingLabel = if (check.kind == SampleCi.Kind.Status) null else timing,
            durationLabel = if (check.kind == SampleCi.Kind.Status) null else duration,
            ticks = running && check.kind != SampleCi.Kind.Status,
            producer = when (check.kind) {
                SampleCi.Kind.Actions -> "GitHub Actions"
                SampleCi.Kind.App -> "Codecov"
                SampleCi.Kind.Status -> "the status provider"
            },
            detailsUrl = "https://github.com/${spot.repo}/pull/${spot.number}/checks",
            source = when (check.kind) {
                SampleCi.Kind.Actions -> CiSource.Actions(jobId(spot), runId(spot), runAttempt = 1)
                SampleCi.Kind.App -> CiSource.App(checkRunId = APP_BASE + spot.number, app = "Codecov")
                SampleCi.Kind.Status -> CiSource.Status
            },
        )
    }

    // --- re-runs -----------------------------------------------------------------------

    private fun choice(spot: Spot, cell: CiCell): CiRerunChoice {
        val running = cell.status == CiStatus.Queued || cell.status == CiStatus.InProgress
        val source = cell.source
        return when {
            source is CiSource.Status -> CiRerunChoice.Unavailable(
                CiNotRerunnable.LegacyStatus,
                "Only ${cell.producer} can run this again.",
            )
            running -> CiRerunChoice.Unavailable(CiNotRerunnable.StillRunning, "It is still running.")
            source is CiSource.App -> CiRerunChoice.Available(
                listOf(CiRerunOption(CiRerun.Suite(SUITE_BASE + spot.number), "Re-run ${source.app}", "Ask ${source.app} to run its checks on #${spot.number} again?")),
            )
            else -> {
                val name = SampleCi.columns(spot.repo)[spot.column].column.label
                val runFailed = SampleCi.columns(spot.repo).indices.any { status(spot.copy(column = it)) == CiStatus.Failure }
                CiRerunChoice.Available(
                    buildList {
                        if (runFailed) add(CiRerunOption(CiRerun.FailedJobs(runId(spot)), "Re-run failed jobs", "Re-run the failed jobs of CI on #${spot.number}?"))
                        add(CiRerunOption(CiRerun.Job(jobId(spot)), "Re-run this job", "Re-run $name on #${spot.number}?"))
                        add(CiRerunOption(CiRerun.AllJobs(runId(spot)), "Re-run all jobs", "Re-run every job of CI on #${spot.number}?"))
                    },
                )
            }
        }
    }

    private fun covered(repo: String, target: CiRerun): List<Spot> {
        val numbers = host.openPulls(repo).map { it.number }
        val columns = SampleCi.columns(repo)
        val all = numbers.flatMap { n -> columns.indices.map { Spot(repo, n, it) } }.filter { cell(it) != null }
        return when (target) {
            is CiRerun.Job -> all.filter { columns[it.column].kind == SampleCi.Kind.Actions && jobId(it) == target.jobId }
            is CiRerun.AllJobs -> all.filter { columns[it.column].kind == SampleCi.Kind.Actions && runId(it) == target.runId }
            is CiRerun.FailedJobs -> all.filter {
                columns[it.column].kind == SampleCi.Kind.Actions && runId(it) == target.runId && status(it) == CiStatus.Failure
            }
            is CiRerun.Suite -> all.filter { columns[it.column].kind == SampleCi.Kind.App && SUITE_BASE + it.number == target.suiteId }
        }
    }

    private fun repoIndex(repo: String) = SamplePulls.repos.indexOf(repo).coerceAtLeast(0) + 1

    private fun jobId(spot: Spot) = repoIndex(spot.repo) * REPO_STRIDE + spot.number * 100L + spot.column

    private fun runId(spot: Spot) = repoIndex(spot.repo) * REPO_STRIDE + spot.number * 10L + 1

    private fun spotOf(repo: String, jobId: Long): Spot? {
        val number = ((jobId % REPO_STRIDE) / 100L).toInt()
        val column = (jobId % 100L).toInt()
        val spot = Spot(repo, number, column)
        return spot.takeIf { jobId(it) == jobId && column < SampleCi.columns(repo).size && cell(it) != null }
    }

    companion object {
        /** Ids of one repository's jobs and runs stay below this (pull request numbers up to 10⁸). */
        private const val REPO_STRIDE = 10_000_000_000L
        private const val APP_BASE = 9_000_000L
        private const val SUITE_BASE = 8_000_000L

        fun statusLabel(status: CiStatus): String = when (status) {
            CiStatus.Queued -> "queued"
            CiStatus.InProgress -> "in progress"
            CiStatus.Success -> "success"
            CiStatus.Failure -> "failure"
            CiStatus.Cancelled -> "cancelled"
            CiStatus.Skipped -> "skipped"
            CiStatus.Neutral -> "neutral"
            CiStatus.TimedOut -> "timed out"
            CiStatus.ActionRequired -> "action required"
        }

        fun roleOf(status: CiStatus): ColorRole = when (status) {
            CiStatus.Success -> ColorRole.Success
            CiStatus.Failure, CiStatus.TimedOut, CiStatus.ActionRequired -> ColorRole.Danger
            CiStatus.Queued, CiStatus.InProgress -> ColorRole.Warning
            CiStatus.Cancelled, CiStatus.Skipped, CiStatus.Neutral -> ColorRole.Neutral
        }

        fun rollup(cells: List<CiCell?>): CiRollup {
            val present = cells.filterNotNull()
            val failing = present.count { roleOf(it.status) == ColorRole.Danger }
            val running = present.count { it.status == CiStatus.Queued || it.status == CiStatus.InProgress }
            val passing = present.count { it.status == CiStatus.Success }
            val other = present.size - failing - running - passing
            val (state, role) = when {
                failing > 0 -> CiRollupState.Failing to ColorRole.Danger
                running > 0 -> CiRollupState.Running to ColorRole.Warning
                passing > 0 -> CiRollupState.Passing to ColorRole.Success
                other > 0 -> CiRollupState.Settled to ColorRole.Neutral
                else -> CiRollupState.Empty to ColorRole.Neutral
            }
            val label = listOfNotNull(
                "$failing failing".takeIf { failing > 0 },
                "$running running".takeIf { running > 0 },
                "$passing passing".takeIf { passing > 0 && failing == 0 && running == 0 },
            ).joinToString(" · ").ifEmpty { if (present.isEmpty()) "no checks" else "settled" }
            return CiRollup(failing, running, passing, other, state, label, role)
        }

        /** `3m 12s`, `45s`, `1h 02m`. */
        fun clockLabel(duration: Duration): String {
            val seconds = duration.seconds
            return when {
                seconds < 60 -> "${seconds}s"
                seconds < 3600 -> "${seconds / 60}m %02ds".format(seconds % 60)
                else -> "${seconds / 3600}h %02dm".format((seconds % 3600) / 60)
            }
        }

        private fun agoLabel(duration: Duration): String {
            val minutes = duration.toMinutes()
            return when {
                minutes < 1 -> "${duration.seconds}s"
                minutes < 60 -> "${minutes}m"
                else -> "${minutes / 60}h"
            }
        }
    }
}
