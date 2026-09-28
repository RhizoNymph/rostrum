package io.github.rhizonymph.rostrum.ui.pr.branch

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.InProgressKind
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.requiresPairing
import io.github.rhizonymph.rostrum.ui.common.Messages
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** The Branch tab's local card: what the paired desktop's clone says. */
sealed interface LocalCardState {
    /** Not asked yet: the tab has not been shown. */
    data object Idle : LocalCardState

    data object Loading : LocalCardState

    /** No desktop is paired (or it forgot this phone). */
    data object NotPaired : LocalCardState

    data class Failed(val error: BackendError) : LocalCardState

    data class Ready(val machine: String, val status: LocalStatus) : LocalCardState
}

data class BranchUiState(
    val local: LocalCardState = LocalCardState.Idle,
    /** The "Stash local changes" switch; starts at the settings default. */
    val autostash: Boolean = false,
    val runningOp: LocalOp? = null,
    val aborting: Boolean = false,
)

/**
 * The desktop half of the Branch tab. Loads lazily ([ensureLoaded]) because
 * the desktop fetches before answering, which takes a moment.
 */
class BranchViewModel(
    private val pr: PrRef,
    private val backend: RostrumBackend,
) : ViewModel() {
    private val _state = MutableStateFlow(BranchUiState())
    val state: StateFlow<BranchUiState> = _state.asStateFlow()
    val messages = Messages()

    private var loadJob: Job? = null
    private var settingsRead = false

    /** Load on first display; later calls do nothing. */
    fun ensureLoaded() {
        if (_state.value.local == LocalCardState.Idle) refresh()
    }

    fun refresh() {
        loadJob?.cancel()
        _state.update { if (it.local is LocalCardState.Ready) it else it.copy(local = LocalCardState.Loading) }
        loadJob = viewModelScope.launch { load() }
    }

    private suspend fun load() {
        if (!settingsRead) {
            val settings = backend.settings()
            if (settings is Outcome.Ok) {
                settingsRead = true
                _state.update { it.copy(autostash = settings.value.autostash) }
            }
        }
        val machine = when (val info = backend.machineInfo()) {
            is Outcome.Ok -> info.value
            is Outcome.Err -> return failed(info.error)
        }
        when (val status = backend.localStatus(pr)) {
            is Outcome.Ok -> _state.update { it.copy(local = LocalCardState.Ready(machine.name, status.value)) }
            is Outcome.Err -> failed(status.error)
        }
    }

    private fun failed(error: BackendError) {
        RostrumLog.w(TAG, "local_status_failed", "pr" to pr, "error" to error::class.simpleName)
        _state.update {
            it.copy(local = if (error.requiresPairing) LocalCardState.NotPaired else LocalCardState.Failed(error))
        }
    }

    fun setAutostash(enabled: Boolean) = _state.update { it.copy(autostash = enabled) }

    /** Run one of the four local operations on the pull request's worktree. */
    fun runOp(op: LocalOp) {
        val current = _state.value
        if (current.runningOp != null || current.aborting) return
        _state.update { it.copy(runningOp = op) }
        viewModelScope.launch {
            when (val result = backend.runLocalJob(pr, op, current.autostash)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "local_job", "pr" to pr, "op" to op, "outcome" to result.value.outcome::class.simpleName)
                    messages.send(result.value.detail)
                    load()
                }
                is Outcome.Err -> handleActionError("local_job_failed", result.error)
            }
            _state.update { it.copy(runningOp = null) }
        }
    }

    /** Abort the stopped rebase or merge. The UI confirms first. */
    fun abort() {
        val current = _state.value
        if (current.aborting || current.runningOp != null) return
        val kind = ((current.local as? LocalCardState.Ready)?.status as? LocalStatus.CheckedOut)?.branch?.inProgress?.kind
        _state.update { it.copy(aborting = true) }
        viewModelScope.launch {
            when (val result = backend.abortLocal(pr)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "local_aborted", "pr" to pr)
                    messages.send("Aborted the ${kind.noun()}")
                    load()
                }
                is Outcome.Err -> handleActionError("local_abort_failed", result.error)
            }
            _state.update { it.copy(aborting = false) }
        }
    }

    private fun handleActionError(event: String, error: BackendError) {
        RostrumLog.w(TAG, event, "pr" to pr, "error" to error::class.simpleName)
        if (error.requiresPairing) _state.update { it.copy(local = LocalCardState.NotPaired) }
        messages.send(error.describe())
    }

    private companion object {
        const val TAG = "RostrumBranch"
    }
}

/** "rebase", "merge", … for messages and button labels. */
fun InProgressKind?.noun(): String = when (this) {
    InProgressKind.Rebase, InProgressKind.Am, null -> "rebase"
    InProgressKind.Merge -> "merge"
    InProgressKind.CherryPick -> "cherry-pick"
    InProgressKind.Revert -> "revert"
    InProgressKind.Bisect -> "bisect"
}
