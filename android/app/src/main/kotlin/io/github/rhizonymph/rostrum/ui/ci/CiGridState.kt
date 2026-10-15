package io.github.rhizonymph.rostrum.ui.ci

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiCheckOutput
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.ui.common.UiState

/**
 * The CI grid screen. [content] is the grid as shown (one repository's
 * section only when the screen was opened from a repository); [sheet] is the
 * cell opened, if any.
 */
data class CiGridUiState(
    val content: UiState<CiGrid> = UiState.Loading,
    val filter: CiGridFilter = CiGridFilter(),
    val refreshing: Boolean = false,
    val sheet: CiSheet? = null,
)

/** The check a sheet is about: the cell of [number] in [column] of [repo]. */
data class CiTarget(
    val repo: String,
    val number: Int,
    val title: String,
    val column: CiColumn,
    val cell: CiCell,
)

/** A cell's sheet: what it shows ([detail]) and where its Retry menu is ([retry]). */
data class CiSheet(
    val target: CiTarget,
    val detail: CiDetail,
    val retry: RetryState = RetryState.Closed,
)

/** What a cell's sheet shows, by who produced the check. */
sealed interface CiDetail {
    /** A GitHub Actions job's log. */
    data class Log(
        val jobId: Long,
        val log: UiState<CiJobLog> = UiState.Loading,
        val view: LogView = LogView(),
        val loadingFull: Boolean = false,
    ) : CiDetail

    /** Another app's check run: its output and annotations. */
    data class Output(val output: UiState<CiCheckOutput> = UiState.Loading) : CiDetail

    /** A legacy commit status: only its link. */
    data object Status : CiDetail
}

/**
 * How the log is being looked at: which groups are folded, the search, and
 * the line to bring into view ([focus], with a serial so asking twice for the
 * same line scrolls again).
 */
data class LogView(
    val collapsed: Set<Int> = emptySet(),
    val query: String = "",
    val matches: List<Int> = emptyList(),
    /** Index into [matches] of the current one, or -1. */
    val current: Int = -1,
    val focus: LogFocus? = null,
)

/** Scroll the log to line index [line]; [serial] grows with every request. */
data class LogFocus(val line: Int, val serial: Int)

/** The Retry menu of a cell's sheet. */
sealed interface RetryState {
    data object Closed : RetryState

    /** Asking the core what this check offers. */
    data object Loading : RetryState

    /** The core's options, or why there are none. */
    data class Menu(val choice: CiRerunChoice) : RetryState

    /** Asking the user to confirm [option] with its prompt. */
    data class Confirm(val option: CiRerunOption) : RetryState

    data class Running(val option: CiRerunOption) : RetryState

    data class Failed(val error: BackendError) : RetryState
}

/** What the CI screen can ask for. The ViewModel implements it; previews use [NoCiGridActions]. */
interface CiGridActions {
    fun refresh()
    fun setNeedsAttention(on: Boolean)
    fun openCell(section: Int, row: Int, column: Int)
    fun closeSheet()
    fun retryDetail()
    fun loadFullLog()
    fun toggleGroup(group: Int)
    fun setLogQuery(query: String)
    fun nextMatch()
    fun previousMatch()
    fun jumpToFirstError()
    fun openRetry()
    fun chooseRerun(option: CiRerunOption)
    fun confirmRerun()
    fun dismissRetry()
}

object NoCiGridActions : CiGridActions {
    override fun refresh() = Unit
    override fun setNeedsAttention(on: Boolean) = Unit
    override fun openCell(section: Int, row: Int, column: Int) = Unit
    override fun closeSheet() = Unit
    override fun retryDetail() = Unit
    override fun loadFullLog() = Unit
    override fun toggleGroup(group: Int) = Unit
    override fun setLogQuery(query: String) = Unit
    override fun nextMatch() = Unit
    override fun previousMatch() = Unit
    override fun jumpToFirstError() = Unit
    override fun openRetry() = Unit
    override fun chooseRerun(option: CiRerunOption) = Unit
    override fun confirmRerun() = Unit
    override fun dismissRetry() = Unit
}
