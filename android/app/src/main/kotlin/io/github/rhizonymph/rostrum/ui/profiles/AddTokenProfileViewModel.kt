package io.github.rhizonymph.rostrum.ui.profiles

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.running
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class AddTokenProfileState(
    /** Optional; blank names the profile after the token's GitHub login. */
    val label: String = "",
    val token: String = "",
    val submit: ActionState = ActionState.Idle,
) {
    val canSubmit: Boolean get() = token.isNotBlank() && !submit.running

    override fun toString(): String = "AddTokenProfileState(label=$label, token=redacted, submit=$submit)"
}

/**
 * "Add a GitHub token profile": check the token with GitHub, keep it in a
 * new profile and switch to it (the root then rebuilds the app on it).
 */
class AddTokenProfileViewModel(
    private val profiles: ProfileManager,
    private val appMessages: Messages = Messages(),
) : ViewModel() {
    private val _state = MutableStateFlow(AddTokenProfileState())
    val state: StateFlow<AddTokenProfileState> = _state.asStateFlow()

    fun onLabelChange(label: String) {
        _state.update { it.copy(label = label, submit = ActionState.Idle) }
    }

    fun onTokenChange(token: String) {
        _state.update { it.copy(token = token, submit = ActionState.Idle) }
    }

    fun add() {
        val current = _state.value
        if (!current.canSubmit) return
        _state.update { it.copy(submit = ActionState.Running) }
        viewModelScope.launch {
            val created = when (val made = profiles.createTokenProfile(current.token, current.label)) {
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "token_profile_failed", "error" to made.error::class.simpleName)
                    _state.update { it.copy(submit = ActionState.Failed(made.error)) }
                    return@launch
                }
                is Outcome.Ok -> made.value
            }
            _state.update { it.copy(token = "", submit = ActionState.Idle) }
            when (profiles.switchTo(created.id)) {
                is Outcome.Ok -> appMessages.send("Added ${created.label}")
                is Outcome.Err -> appMessages.send("Added ${created.label}. Switch to it from the profile menu.")
            }
        }
    }

    private companion object {
        const val TAG = "RostrumProfiles"
    }
}
