package io.github.rhizonymph.rostrum.ui.desktopconfig

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.ConfigPushResult
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.running
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** The "Send settings to <machine>" sheet. */
sealed interface PushSheetState {
    data object Closed : PushSheetState

    data object Loading : PushSheetState

    data class Failed(val error: BackendError) : PushSheetState

    /**
     * The desktop's settings at [preview]'s revision and what sending would
     * change there. [stale]: a send found the desktop changed since it was
     * last shown, so this is the fresh difference.
     */
    data class Ready(
        val preview: DesktopConfigPreview,
        val stale: Boolean = false,
        val send: ActionState = ActionState.Idle,
    ) : PushSheetState {
        val changes: List<ConfigChangeView> get() = preview.pushChanges.map(ConfigChangeText::view)

        val nothingToSend: Boolean get() = preview.pushChanges.isEmpty()
    }
}

interface PushSheetActions {
    fun open()
    fun retry()
    fun send()
    fun dismiss()
}

object NoPushSheetActions : PushSheetActions {
    override fun open() = Unit
    override fun retry() = Unit
    override fun send() = Unit
    override fun dismiss() = Unit
}

/**
 * "Send settings to <machine>", from the Desktop tab and Settings: preview
 * what this profile's settings would change on the desktop, then replace
 * them there. The push names the previewed revision, so a desktop changed
 * meanwhile is never overwritten unseen: its fresh difference is shown and
 * the user sends again. [messages] carries the snackbar.
 */
class PushSettingsViewModel(private val backend: RostrumBackend) : ViewModel(), PushSheetActions {
    private val _state = MutableStateFlow<PushSheetState>(PushSheetState.Closed)
    val state: StateFlow<PushSheetState> = _state.asStateFlow()
    val messages = Messages()

    private var job: Job? = null

    override fun open() {
        if (_state.value != PushSheetState.Closed) return
        load(stale = false)
    }

    override fun retry() {
        if (_state.value is PushSheetState.Failed) load(stale = false)
    }

    private fun load(stale: Boolean) {
        _state.value = PushSheetState.Loading
        job?.cancel()
        job = viewModelScope.launch { fetchPreview(stale) }
    }

    private suspend fun fetchPreview(stale: Boolean) {
        _state.value = PushSheetState.Loading
        _state.value = when (val preview = backend.desktopConfig().logErr(TAG, "push_preview_failed")) {
            is Outcome.Ok -> PushSheetState.Ready(preview.value, stale = stale)
            is Outcome.Err -> PushSheetState.Failed(preview.error)
        }
    }

    override fun send() {
        val ready = _state.value as? PushSheetState.Ready ?: return
        if (ready.send.running || ready.nothingToSend) return
        _state.value = ready.copy(send = ActionState.Running)
        job = viewModelScope.launch {
            when (val pushed = backend.pushConfigToDesktop(ready.preview.revision).logErr(TAG, "push_failed")) {
                is Outcome.Ok -> when (val result = pushed.value) {
                    is ConfigPushResult.Applied -> {
                        RostrumLog.i(TAG, "settings_pushed", "machine" to result.desktop.machine, "revision" to result.desktop.revision)
                        _state.value = PushSheetState.Closed
                        messages.send(PushConfigText.sent(result.desktop.machine))
                    }
                    is ConfigPushResult.Changed -> {
                        RostrumLog.i(TAG, "push_refused_changed", "revision" to result.desktop.revision)
                        _state.value = PushSheetState.Ready(result.desktop, stale = true)
                    }
                }
                is Outcome.Err -> {
                    val error = pushed.error
                    if (error is BackendError.RemoteApi && error.code == RemoteErrorCode.ConfigChanged) {
                        fetchPreview(stale = true)
                    } else {
                        _state.value = ready.copy(send = ActionState.Failed(error))
                    }
                }
            }
        }
    }

    override fun dismiss() {
        val current = _state.value
        if (current is PushSheetState.Ready && current.send.running) return
        job?.cancel()
        _state.value = PushSheetState.Closed
    }

    private companion object {
        const val TAG = "RostrumConfigPush"
    }
}
