package io.github.rhizonymph.rostrum.ui.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.desktopconfig.DesktopConfigCopier
import io.github.rhizonymph.rostrum.ui.desktopconfig.DesktopConfigText
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch

/** The "Copy settings from <machine>" sheet. */
sealed interface CopySheetState {
    data object Closed : CopySheetState

    data object Loading : CopySheetState

    data class Failed(val error: BackendError) : CopySheetState

    data class Ready(
        val preview: DesktopConfigPreview,
        /** What copying would change here, one line each. */
        val changes: List<String> = emptyList(),
        val copy: ActionState = ActionState.Idle,
    ) : CopySheetState {
        /** "2 repositories will be removed from this profile.", when any are. */
        val removalWarning: String? get() = DesktopConfigText.removalWarning(preview.removed.size)
    }
}

interface CopySheetActions {
    fun open()
    fun retry()
    fun replace()
    fun dismiss()
}

object NoCopySheetActions : CopySheetActions {
    override fun open() = Unit
    override fun retry() = Unit
    override fun replace() = Unit
    override fun dismiss() = Unit
}

/**
 * Settings › Desktop › "Copy settings from <machine>": preview the desktop's
 * settings, then replace this phone's with them. [copied] fires after a copy
 * so Settings reloads its repository list; [messages] carries the snackbar.
 */
class DesktopConfigSheetViewModel(backend: RostrumBackend) : ViewModel(), CopySheetActions {
    private val copier = DesktopConfigCopier(backend)
    private val _state = MutableStateFlow<CopySheetState>(CopySheetState.Closed)
    val state: StateFlow<CopySheetState> = _state.asStateFlow()
    val messages = Messages()

    private val copiedEvents = Channel<Unit>(Channel.BUFFERED)
    val copied: Flow<Unit> = copiedEvents.receiveAsFlow()

    private var job: Job? = null

    override fun open() {
        if (_state.value != CopySheetState.Closed) return
        load()
    }

    override fun retry() {
        if (_state.value is CopySheetState.Failed) load()
    }

    private fun load() {
        _state.value = CopySheetState.Loading
        job?.cancel()
        job = viewModelScope.launch {
            _state.value = when (val preview = copier.preview()) {
                is Outcome.Ok -> CopySheetState.Ready(preview.value.preview, preview.value.changes)
                is Outcome.Err -> CopySheetState.Failed(preview.error)
            }
        }
    }

    override fun replace() {
        val ready = _state.value as? CopySheetState.Ready ?: return
        if (ready.copy.running || !ready.preview.changesAnything) return
        _state.value = ready.copy(copy = ActionState.Running)
        job = viewModelScope.launch {
            when (val copied = copier.copy(ready.preview.machine)) {
                is Outcome.Ok -> {
                    _state.value = CopySheetState.Closed
                    messages.send(copied.value)
                    copiedEvents.trySend(Unit)
                }
                is Outcome.Err -> _state.value = ready.copy(copy = ActionState.Failed(copied.error))
            }
        }
    }

    override fun dismiss() {
        val current = _state.value
        if (current is CopySheetState.Ready && current.copy.running) return
        job?.cancel()
        _state.value = CopySheetState.Closed
    }
}
