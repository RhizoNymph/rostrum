package io.github.rhizonymph.rostrum.ui.profiles

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.ui.common.Messages
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SwitcherUiState(
    val rows: List<ProfileRow> = emptyList(),
    /** The profile being switched to. */
    val switching: ProfileId? = null,
    /** The last switch failed. */
    val error: BackendError? = null,
)

/**
 * The profile switcher sheet: every profile, the active one checked. Picking
 * another makes it active; the root then rebuilds the app on it (which also
 * closes the sheet), and [appMessages] says so.
 */
class ProfileSwitcherViewModel(
    private val profiles: ProfileManager,
    private val appMessages: Messages = Messages(),
) : ViewModel() {
    private val _state = MutableStateFlow(SwitcherUiState())
    val state: StateFlow<SwitcherUiState> = _state.asStateFlow()

    init {
        viewModelScope.launch {
            profiles.state.collect { profilesState -> _state.update { it.copy(rows = profileRows(profilesState)) } }
        }
    }

    /** Switch to [id]. Returns `false` when there is nothing to do (it is already active, or a switch is running). */
    fun switchTo(id: ProfileId): Boolean {
        val current = _state.value
        if (current.switching != null || current.rows.any { it.id == id && it.active }) return false
        _state.update { it.copy(switching = id, error = null) }
        viewModelScope.launch {
            when (val switched = profiles.switchTo(id)) {
                is Outcome.Ok -> {
                    appMessages.send(ProfileText.switched(switched.value))
                    _state.update { it.copy(switching = null) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "profile_switch_failed", "error" to switched.error::class.simpleName)
                    _state.update { it.copy(switching = null, error = switched.error) }
                }
            }
        }
        return true
    }

    private companion object {
        const val TAG = "RostrumProfiles"
    }
}
