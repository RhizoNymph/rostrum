package io.github.rhizonymph.rostrum.data.model

/*
 * The CI grid (`ci/types.rs`): every open pull request's checks in one
 * matrix, with job logs, another app's check output, and re-runs. Ids are
 * GitHub's 64-bit ids as [Long].
 */

/** The grid's own narrowing, on top of the feed's filter. */
data class CiGridFilter(
    /** Only pull requests with something failing or still running. */
    val needsAttention: Boolean = false,
)

/**
 * Why the grid fetches checks: the core sends only what each calls for,
 * by the desktop's skip rules (`ci/fetch.rs`).
 */
enum class CiTrigger {
    /** The grid opened: only what could have changed since the last fetch. */
    Opened,

    /** The 15 s watch while something runs: only what is queued or running. */
    Watch,

    /** The user asked (pull to refresh): everything. */
    Explicit,
}

/** One check's state, folded from GitHub's status and conclusion. */
enum class CiStatus { Queued, InProgress, Success, Failure, Cancelled, Skipped, Neutral, TimedOut, ActionRequired }

/** A column: an Actions job's workflow and name, or an app's or status's name alone. */
data class CiCheckKey(val workflow: String?, val name: String)

data class CiColumn(
    val key: CiCheckKey,
    /** `CI / build`, or `Coverage`. */
    val label: String,
)

/** Who produced a check, and the ids its log, output and re-runs take. */
sealed interface CiSource {
    /** A GitHub Actions job: its log is [jobId]'s. */
    data class Actions(val jobId: Long, val runId: Long, val runAttempt: Int) : CiSource

    /** Another app's check run: its output is [checkRunId]'s. */
    data class App(val checkRunId: Long, val app: String) : CiSource

    /** A legacy commit status: only its link. */
    data object Status : CiSource
}

/** One cell: a check on one pull request. */
data class CiCell(
    val status: CiStatus,
    /** `in progress`, `failure`, … */
    val statusLabel: String,
    val role: ColorRole,
    /** `3m 12s`, `queued 2m 00s`, `finished 14m ago`, as of when the grid was built. */
    val timingLabel: String?,
    /** `running for 3m 12s`, `took 4m 03s`. */
    val durationLabel: String?,
    /** The labels change every second. */
    val ticks: Boolean,
    /** `GitHub Actions`, the app's name, or `the status provider`. */
    val producer: String,
    val detailsUrl: String?,
    val source: CiSource,
)

enum class CiRollupState {
    Failing,
    Running,
    Passing,

    /** Only skipped, cancelled or neutral checks. */
    Settled,

    /** No checks at all. */
    Empty,
}

/** A pull request's checks in one word. */
data class CiRollup(
    val failing: Int,
    val running: Int,
    val passing: Int,
    val other: Int,
    val state: CiRollupState,
    /** `2 failing · 1 running`, or `no checks`. */
    val label: String,
    val role: ColorRole,
)

/** Where a row sits in a stack the feed groups. */
enum class CiStackPlace { Bottom, Middle, Top, Only }

data class CiRow(
    /** `owner/name`: rows of every repository share one grid. */
    val repo: String,
    val number: Int,
    val title: String,
    /** The head commit, seven characters. */
    val headSha: String,
    val rollup: CiRollup,
    /** One per grid column; `null` is "not run" on this pull request. */
    val cells: List<CiCell?>,
    val stack: CiStackPlace?,
    /** Whether this pull request's checks have been fetched at all. */
    val fetched: Boolean,
    /** GitHub reported more checks than were fetched. */
    val truncated: Boolean,
)

/**
 * Adjacent columns of one workflow, `start until end` into [CiGrid.columns],
 * for a header naming the workflow once above its jobs. The groups partition
 * the columns in order.
 */
data class CiColumnGroup(
    /** `null` for apps' checks and legacy statuses. */
    val workflow: String?,
    val start: Int,
    val end: Int,
)

/** A repository the grid covers: its checks' fetch state and row counts. */
data class CiRepo(
    val repo: String,
    val load: RepoLoad,
    /** Rows the grid shows for this repository. */
    val rows: Int,
    /** Rows the grid filter removed. */
    val hidden: Int,
)

/** A line of the grid flattened for one list. The column header is not a line: one header sits above the list. */
sealed interface CiLine {
    /** Heads a stack's rows: "Stack · 3 PRs". */
    data class Stack(val members: Int) : CiLine

    /** Index into [CiGrid.rows]. */
    data class Row(val row: Int) : CiLine
}

/**
 * The pull requests × checks matrix: every open, non-draft pull request of
 * the watched repositories as rows, in the feed's order, one column set for
 * all of them.
 */
data class CiGrid(
    /** Every column, ordered workflow then name. */
    val columns: List<CiColumn>,
    val groups: List<CiColumnGroup>,
    val rows: List<CiRow>,
    /** Each repository the feed shows, in the feed's order. */
    val repos: List<CiRepo>,
    val lines: List<CiLine>,
    /** Rows the grid filter removed, across every repository. */
    val hidden: Int,
    /** Some cell's labels change every second: rebuild ([ciGrid], no network) once a second while shown. */
    val ticks: Boolean,
    /** Something is queued or running: re-fetch ([refreshCi]) every 15 s while shown. */
    val anyRunning: Boolean,
) {
    companion object {
        val Empty = CiGrid(
            columns = emptyList(), groups = emptyList(), rows = emptyList(), repos = emptyList(), lines = emptyList(),
            hidden = 0, ticks = false, anyRunning = false,
        )
    }
}

enum class CiLineKind {
    Plain,

    /** The title of a collapsible group. */
    GroupHeader,
    Error,
    Warning,
    Notice,
    Debug,

    /** A `[command]` echo of what the runner executed. */
    Command,
}

data class CiLogLine(
    /** The line's number in the full log, from 1. */
    val number: Int,
    /** Timestamp, ANSI and workflow-command markers removed. */
    val text: String,
    val kind: CiLineKind,
)

/** A collapsible group: [header] and every line up to [end] (exclusive), as indices into the lines. */
data class CiLogGroup(val title: String, val header: Int, val end: Int)

/** One step of the job: lines `start until end`. */
data class CiLogStep(val title: String, val start: Int, val end: Int)

/** An Actions job's log, parsed. */
data class CiJobLog(
    val lines: List<CiLogLine>,
    val groups: List<CiLogGroup>,
    val steps: List<CiLogStep>,
    /** Index into [lines] of the first error. */
    val firstError: Int?,
    /** Index into [steps] of the step holding it. */
    val failingStep: Int?,
    /** Indices into [groups] to start collapsed. */
    val collapsed: List<Int>,
    /** Lines dropped from the top of a long log. */
    val dropped: Int,
    /** The log was cut to its tail; asking with `full = true` has it all. */
    val truncated: Boolean,
)

enum class CiAnnotationLevel { Notice, Warning, Failure }

/** A note a check run attached to lines of a file. */
data class CiAnnotation(
    val path: String,
    val startLine: Int,
    val endLine: Int,
    val level: CiAnnotationLevel,
    val title: String?,
    val message: String,
    /** `src/lib.rs:12` or `src/lib.rs:12-14`. */
    val location: String,
)

/** Another app's check run: its output as markdown, and its annotations. */
data class CiCheckOutput(
    val title: String?,
    val summary: List<MdBlock>,
    val text: List<MdBlock>,
    val annotations: List<CiAnnotation>,
)

/** A re-run the user can ask for. */
sealed interface CiRerun {
    data class Job(val jobId: Long) : CiRerun

    /** Every failed or cancelled job of a workflow run. */
    data class FailedJobs(val runId: Long) : CiRerun

    data class AllJobs(val runId: Long) : CiRerun

    /** Ask another app to run its check suite again. */
    data class Suite(val suiteId: Long) : CiRerun
}

data class CiRerunOption(
    val rerun: CiRerun,
    /** `Re-run failed jobs`. */
    val label: String,
    /** The confirmation's question, naming what restarts. */
    val confirmPrompt: String,
)

/** Why a check offers no re-run. */
enum class CiNotRerunnable {
    /** Its workflow run (or the check) has not finished. */
    StillRunning,

    /** A legacy commit status: only the service that posted it can re-run it. */
    LegacyStatus,

    /** Another app's check without a suite to re-request. */
    NoSuite,
}

/** What a check offers, first option primary. */
sealed interface CiRerunChoice {
    data class Available(val options: List<CiRerunOption>) : CiRerunChoice

    data class Unavailable(val reason: CiNotRerunnable, val message: String) : CiRerunChoice
}
