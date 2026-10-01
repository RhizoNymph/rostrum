package io.github.rhizonymph.rostrum.ui.desktop

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncRun
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.isPaired
import io.github.rhizonymph.rostrum.data.valueOrNull
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.toUiState
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Clock

/** What the Desktop tab can ask for; implemented by [DesktopViewModel], no-op in previews. */
interface DesktopActions {
    fun refresh()
    fun abort(session: HandoffSession)
    fun startSync(op: SyncAllOp)
    fun setAutostash(enabled: Boolean)
    fun unpair()
    fun forgetDesktop()
    fun dismissUnpairFailure()
    fun fetchGitHubToken()
    fun onCopied()
}

object NoDesktopActions : DesktopActions {
    override fun refresh() = Unit
    override fun abort(session: HandoffSession) = Unit
    override fun startSync(op: SyncAllOp) = Unit
    override fun setAutostash(enabled: Boolean) = Unit
    override fun unpair() = Unit
    override fun forgetDesktop() = Unit
    override fun dismissUnpairFailure() = Unit
    override fun fetchGitHubToken() = Unit
    override fun onCopied() = Unit
}

/**
 * The paired desktop: its machine card, the tmux sessions holding stopped
 * rebases, and "sync all" (started here, polled every [pollMillis] until it
 * finishes). Reloads when the pairing changes.
 */
class DesktopViewModel(
    private val backend: RostrumBackend,
    private val session: SessionRepository,
    val clock: Clock,
    private val pollMillis: Long = 1_000,
) : ViewModel(), DesktopActions {
    private val _state = MutableStateFlow(DesktopState())
    val state: StateFlow<DesktopState> = _state.asStateFlow()
    val messages = Messages()

    private var loadJob: Job? = null
    private var pollJob: Job? = null

    init {
        viewModelScope.launch {
            session.state.map { it.isPaired }.distinctUntilChanged().collect { reload() }
        }
    }

    override fun refresh() {
        if (_state.value.page !is DesktopPage.Loaded) _state.update { it.copy(page = DesktopPage.Loading) }
        reload()
    }

    private fun reload() {
        loadJob?.cancel()
        loadJob = viewModelScope.launch { load() }
    }

    private suspend fun load() {
        if (!session.state.value.isPaired) {
            pollJob?.cancel()
            _state.update { it.copy(page = DesktopPage.NotPaired) }
            return
        }
        val machine = when (val outcome = backend.machineInfo().logErr(TAG, "machine_info_failed")) {
            is Outcome.Err -> {
                val page = if (outcome.error == BackendError.NotPaired) DesktopPage.NotPaired else DesktopPage.Unreachable(outcome.error)
                _state.update { it.copy(page = page) }
                return
            }
            is Outcome.Ok -> outcome.value
        }
        val handoffs = backend.handoffs().logErr(TAG, "handoffs_failed").toUiState()
        val autostash = backend.settings().valueOrNull()?.autostash ?: machine.autostash
        val status = backend.syncAllStatus().logErr(TAG, "sync_status_failed").valueOrNull()
        val previous = (_state.value.page as? DesktopPage.Loaded)?.content
        val running = status?.takeIf { it.running }
        _state.update {
            it.copy(
                page = DesktopPage.Loaded(
                    DesktopContent(
                        machine = machine,
                        handoffs = handoffs,
                        autostash = autostash,
                        sync = running?.let { run -> SyncActivity.Running(run) } ?: SyncActivity.Idle,
                        lastRun = status?.takeUnless { run -> run.running } ?: previous?.lastRun,
                        aborting = previous?.aborting.orEmpty(),
                    ),
                ),
            )
        }
        if (running != null) poll()
    }

    private fun updateContent(transform: (DesktopContent) -> DesktopContent) {
        _state.update { state ->
            val page = state.page as? DesktopPage.Loaded ?: return@update state
            state.copy(page = DesktopPage.Loaded(transform(page.content)))
        }
    }

    private fun content(): DesktopContent? = (_state.value.page as? DesktopPage.Loaded)?.content

    private suspend fun reloadHandoffs() {
        val handoffs = backend.handoffs().logErr(TAG, "handoffs_failed").toUiState()
        updateContent { it.copy(handoffs = handoffs) }
    }

    override fun abort(session: HandoffSession) {
        val pr = session.pr ?: return
        if (content()?.aborting?.contains(session.session) == true) return
        updateContent { it.copy(aborting = it.aborting + session.session) }
        viewModelScope.launch {
            when (val result = backend.abortLocal(pr)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "handoff_aborted", "pr" to pr, "session" to session.session)
                    messages.send("Aborted the stopped operation in ${session.headRef ?: pr.toString()}")
                    reloadHandoffs()
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "handoff_abort_failed", "pr" to pr, "error" to result.error::class.simpleName)
                    messages.send(result.error.describe())
                }
            }
            updateContent { it.copy(aborting = it.aborting - session.session) }
        }
    }

    override fun startSync(op: SyncAllOp) {
        val content = content() ?: return
        if (content.sync != SyncActivity.Idle) return
        updateContent { it.copy(sync = SyncActivity.Starting(op)) }
        viewModelScope.launch {
            when (val started = backend.startSyncAll(op, content.autostash)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "sync_all_started", "op" to op, "entries" to started.value.entries.size)
                    if (started.value.running) {
                        updateContent { it.copy(sync = SyncActivity.Running(started.value)) }
                        poll()
                    } else {
                        finish(started.value)
                    }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "sync_all_start_failed", "op" to op, "error" to started.error::class.simpleName)
                    updateContent { it.copy(sync = SyncActivity.Idle) }
                    messages.send(started.error.describe())
                }
            }
        }
    }

    private fun poll() {
        pollJob?.cancel()
        pollJob = viewModelScope.launch {
            while (true) {
                delay(pollMillis)
                when (val status = backend.syncAllStatus()) {
                    is Outcome.Err -> {
                        RostrumLog.w(TAG, "sync_all_poll_failed", "error" to status.error::class.simpleName)
                        updateContent { it.copy(sync = SyncActivity.Idle) }
                        messages.send("Lost track of the sync: ${status.error.describe()}")
                        return@launch
                    }
                    is Outcome.Ok -> {
                        val run = status.value
                        when {
                            run == null -> {
                                updateContent { it.copy(sync = SyncActivity.Idle) }
                                return@launch
                            }
                            run.running -> updateContent { it.copy(sync = SyncActivity.Running(run)) }
                            else -> {
                                finish(run)
                                return@launch
                            }
                        }
                    }
                }
            }
        }
    }

    private suspend fun finish(run: SyncRun) {
        RostrumLog.i(TAG, "sync_all_finished", "op" to run.op, "summary" to run.progressText)
        updateContent { it.copy(sync = SyncActivity.Idle, lastRun = run) }
        messages.send("${runTitle(run.op)}: ${run.progressText}")
        reloadHandoffs()
    }

    override fun setAutostash(enabled: Boolean) {
        viewModelScope.launch {
            when (val saved = backend.setAutostash(enabled)) {
                is Outcome.Ok -> updateContent { it.copy(autostash = saved.value.autostash) }
                is Outcome.Err -> messages.send(saved.error.describe())
            }
        }
    }

    override fun unpair() {
        if (_state.value.unpairing) return
        val name = content()?.machine?.name ?: "the desktop"
        _state.update { it.copy(unpairing = true, unpairFailure = null) }
        viewModelScope.launch {
            when (val result = session.unpair()) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "unpaired", "machine" to name)
                    messages.send("Unpaired from $name")
                    _state.update { it.copy(unpairing = false) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "unpair_failed", "error" to result.error::class.simpleName)
                    _state.update { it.copy(unpairing = false, unpairFailure = result.error) }
                }
            }
        }
    }

    override fun forgetDesktop() {
        _state.update { it.copy(unpairFailure = null) }
        viewModelScope.launch { session.forgetDesktop() }
    }

    override fun dismissUnpairFailure() {
        _state.update { it.copy(unpairFailure = null) }
    }

    override fun fetchGitHubToken() {
        val name = content()?.machine?.name ?: "the desktop"
        viewModelScope.launch {
            when (val result = session.refreshTokenFromDesktop()) {
                is Outcome.Ok -> messages.send("Signed in with $name's GitHub token")
                is Outcome.Err -> messages.send(result.error.describe())
            }
        }
    }

    override fun onCopied() {
        messages.send("Copied")
    }

    private companion object {
        const val TAG = "RostrumDesktop"
    }
}
