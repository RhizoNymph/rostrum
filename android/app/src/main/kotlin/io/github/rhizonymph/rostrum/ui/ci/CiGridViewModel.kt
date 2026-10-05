package io.github.rhizonymph.rostrum.ui.ci

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.toUiState
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * The CI grid: every open pull request's checks (or one repository's, when
 * [repo] is set), a cell's log or output, and re-runs.
 *
 * Opening shows the checks the core holds at once, then fetches. While the
 * screen is started ([start]…[stop]), the grid is rebuilt from what is held
 * every [tickMillis] when some label ticks, and fetched again every
 * [pollMillis] when something is queued or running, as the core advises.
 */
class CiGridViewModel(
    private val backend: RostrumBackend,
    val repo: String?,
    private val tickMillis: Long = 1_000,
    private val pollMillis: Long = 15_000,
) : ViewModel(), CiGridActions {
    private val _state = MutableStateFlow(CiGridUiState())
    val state: StateFlow<CiGridUiState> = _state.asStateFlow()
    val messages = Messages()

    private var visible: Job? = null
    private var fetchJob: Job? = null
    private var detailJob: Job? = null
    private var retryJob: Job? = null
    private var focusSerial = 0

    private val filter: CiGridFilter get() = _state.value.filter
    private val grid: CiGrid? get() = _state.value.content.dataOrNull()

    init {
        viewModelScope.launch {
            rebuild()
            fetch()
        }
    }

    // --- the grid ----------------------------------------------------------------------

    private suspend fun rebuild() {
        when (val built = backend.ciGrid(filter).logErr(TAG, "ci_grid_failed")) {
            is Outcome.Ok -> show(built.value)
            is Outcome.Err -> if (grid == null) _state.update { it.copy(content = UiState.Error(built.error)) }
        }
    }

    private suspend fun fetch() {
        _state.update { it.copy(refreshing = true) }
        val fetched = (if (repo == null) backend.refreshCi(filter) else backend.refreshCiRepo(repo, filter))
            .logErr(TAG, "ci_fetch_failed", "repo" to repo)
        when (fetched) {
            is Outcome.Ok -> show(fetched.value)
            is Outcome.Err -> {
                val error = fetched.error
                if (grid == null) _state.update { it.copy(content = UiState.Error(error)) } else messages.send(error.describe())
            }
        }
        _state.update { it.copy(refreshing = false) }
    }

    private fun show(grid: CiGrid) {
        _state.update { it.copy(content = UiState.Loaded(CiGridLayout.only(grid, repo))) }
    }

    override fun refresh() {
        if (fetchJob?.isActive == true) return
        _state.update { it.copy(refreshing = true) }
        fetchJob = viewModelScope.launch { fetch() }
    }

    override fun setNeedsAttention(on: Boolean) {
        if (filter.needsAttention == on) return
        _state.update { it.copy(filter = CiGridFilter(needsAttention = on)) }
        viewModelScope.launch { rebuild() }
    }

    /** The screen is on show: keep the labels and the running checks current. */
    fun start() {
        if (visible?.isActive == true) return
        visible = viewModelScope.launch {
            launch {
                while (isActive) {
                    delay(tickMillis)
                    if (grid?.ticks == true) rebuild()
                }
            }
            launch {
                while (isActive) {
                    delay(pollMillis)
                    if (grid?.anyRunning == true && fetchJob?.isActive != true) fetch()
                }
            }
        }
    }

    /** The screen is hidden: stop ticking and polling. */
    fun stop() {
        visible?.cancel()
        visible = null
    }

    // --- a cell ------------------------------------------------------------------------

    override fun openCell(section: Int, row: Int, column: Int) {
        val shown = grid ?: return
        val sec = shown.sections.getOrNull(section) ?: return
        val pull = sec.rows.getOrNull(row) ?: return
        val cell = pull.cells.getOrNull(column) ?: return
        val target = CiTarget(sec.repo, pull.number, pull.title, sec.columns[column], cell)
        val detail = when (val source = cell.source) {
            is CiSource.Actions -> CiDetail.Log(source.jobId)
            is CiSource.App -> CiDetail.Output()
            CiSource.Status -> CiDetail.Status
        }
        _state.update { it.copy(sheet = CiSheet(target, detail)) }
        loadDetail()
    }

    override fun retryDetail() = loadDetail()

    private fun loadDetail() {
        val sheet = _state.value.sheet ?: return
        detailJob?.cancel()
        when (val detail = sheet.detail) {
            is CiDetail.Log -> {
                updateDetail<CiDetail.Log> { it.copy(log = UiState.Loading) }
                detailJob = viewModelScope.launch {
                    val log = backend.jobLog(sheet.target.repo, detail.jobId, full = false).logErr(TAG, "job_log_failed")
                    updateDetail<CiDetail.Log> { current ->
                        current.copy(log = log.toUiState(), view = (log as? Outcome.Ok)?.value?.let(::freshView) ?: current.view)
                    }
                }
            }
            is CiDetail.Output -> {
                updateDetail<CiDetail.Output> { CiDetail.Output() }
                val checkRunId = (sheet.target.cell.source as CiSource.App).checkRunId
                detailJob = viewModelScope.launch {
                    val output = backend.checkOutput(sheet.target.repo, checkRunId).logErr(TAG, "check_output_failed")
                    updateDetail<CiDetail.Output> { CiDetail.Output(output.toUiState()) }
                }
            }
            CiDetail.Status -> Unit
        }
    }

    private fun freshView(log: CiJobLog) = LogView(collapsed = log.collapsed.toSet())

    override fun closeSheet() {
        detailJob?.cancel()
        retryJob?.cancel()
        _state.update { it.copy(sheet = null) }
    }

    override fun loadFullLog() {
        val sheet = _state.value.sheet ?: return
        val detail = sheet.detail as? CiDetail.Log ?: return
        if (detail.loadingFull || detail.log.dataOrNull()?.truncated != true) return
        updateDetail<CiDetail.Log> { it.copy(loadingFull = true) }
        detailJob = viewModelScope.launch {
            when (val full = backend.jobLog(sheet.target.repo, detail.jobId, full = true).logErr(TAG, "full_log_failed")) {
                is Outcome.Ok -> updateDetail<CiDetail.Log> {
                    it.copy(log = UiState.Loaded(full.value), view = freshView(full.value), loadingFull = false)
                }
                is Outcome.Err -> {
                    updateDetail<CiDetail.Log> { it.copy(loadingFull = false) }
                    messages.send(full.error.describe())
                }
            }
        }
    }

    // --- the log viewer ----------------------------------------------------------------

    private inline fun updateLog(crossinline transform: (CiJobLog, LogView) -> LogView) {
        updateDetail<CiDetail.Log> { detail ->
            val log = detail.log.dataOrNull() ?: return@updateDetail detail
            detail.copy(view = transform(log, detail.view))
        }
    }

    override fun toggleGroup(group: Int) = updateLog { _, view ->
        view.copy(collapsed = if (group in view.collapsed) view.collapsed - group else view.collapsed + group)
    }

    override fun setLogQuery(query: String) = updateLog { log, view ->
        val matches = LogLayout.matches(log, query)
        val base = view.copy(query = query, matches = matches, current = if (matches.isEmpty()) -1 else 0)
        matches.firstOrNull()?.let { reveal(log, base, it) } ?: base
    }

    override fun nextMatch() = stepMatch(+1)

    override fun previousMatch() = stepMatch(-1)

    private fun stepMatch(step: Int) = updateLog { log, view ->
        if (view.matches.isEmpty()) return@updateLog view
        val next = Math.floorMod(view.current + step, view.matches.size)
        reveal(log, view.copy(current = next), view.matches[next])
    }

    override fun jumpToFirstError() = updateLog { log, view ->
        log.firstError?.let { reveal(log, view, it) } ?: view
    }

    /** Unfold [line]'s group and bring it into view. */
    private fun reveal(log: CiJobLog, view: LogView, line: Int): LogView {
        val group = LogLayout.groupOf(log, line)
        val collapsed = if (group != null && log.groups[group].header != line) view.collapsed - group else view.collapsed
        return view.copy(collapsed = collapsed, focus = LogFocus(line, ++focusSerial))
    }

    // --- re-runs -----------------------------------------------------------------------

    override fun openRetry() {
        val sheet = _state.value.sheet ?: return
        updateRetry(RetryState.Loading)
        retryJob?.cancel()
        retryJob = viewModelScope.launch {
            val target = sheet.target
            val choice = backend.rerunTargets(target.repo, target.number, target.column.key).logErr(TAG, "rerun_targets_failed")
            updateRetry(
                when (choice) {
                    is Outcome.Ok -> RetryState.Menu(choice.value)
                    is Outcome.Err -> RetryState.Failed(choice.error)
                },
            )
        }
    }

    override fun chooseRerun(option: CiRerunOption) {
        if (_state.value.sheet?.retry !is RetryState.Menu) return
        updateRetry(RetryState.Confirm(option))
    }

    override fun confirmRerun() {
        val sheet = _state.value.sheet ?: return
        val option = (sheet.retry as? RetryState.Confirm)?.option ?: return
        updateRetry(RetryState.Running(option))
        retryJob = viewModelScope.launch {
            when (val rerun = backend.rerun(sheet.target.repo, option.rerun).logErr(TAG, "rerun_failed")) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "rerun_requested", "repo" to sheet.target.repo, "pr" to sheet.target.number, "label" to option.label)
                    closeSheet()
                    messages.send(CiText.rerunRequested(option, sheet.target.number))
                    rebuild()
                }
                is Outcome.Err -> updateRetry(RetryState.Failed(rerun.error))
            }
        }
    }

    override fun dismissRetry() {
        val retry = _state.value.sheet?.retry
        if (retry is RetryState.Running) return
        retryJob?.cancel()
        updateRetry(RetryState.Closed)
    }

    // --- helpers -----------------------------------------------------------------------

    private fun updateRetry(retry: RetryState) {
        _state.update { state -> state.copy(sheet = state.sheet?.copy(retry = retry)) }
    }

    private inline fun <reified D : CiDetail> updateDetail(crossinline transform: (D) -> CiDetail) {
        _state.update { state ->
            val sheet = state.sheet ?: return@update state
            val detail = sheet.detail as? D ?: return@update state
            state.copy(sheet = sheet.copy(detail = transform(detail)))
        }
    }

    override fun onCleared() {
        stop()
    }

    private companion object {
        const val TAG = "RostrumCi"
    }
}
