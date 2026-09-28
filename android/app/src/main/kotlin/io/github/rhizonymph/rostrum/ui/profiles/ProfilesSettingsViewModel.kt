package io.github.rhizonymph.rostrum.ui.profiles

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.running
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** The rename dialog for [id]. */
data class RenameDialog(val id: ProfileId, val text: String, val action: ActionState = ActionState.Idle) {
    val canSave: Boolean get() = text.isNotBlank() && !action.running
}

/** The removal confirmation for [id], with the sentences it shows. */
data class RemoveDialog(val id: ProfileId, val title: String, val body: String, val action: ActionState = ActionState.Idle)

data class ProfilesSectionState(
    val rows: List<ProfileRow> = emptyList(),
    val rename: RenameDialog? = null,
    val remove: RemoveDialog? = null,
)

interface ProfilesSectionActions {
    fun startRename(id: ProfileId)
    fun onRenameChange(text: String)
    fun saveRename()
    fun askRemove(id: ProfileId)
    fun confirmRemove()
    fun dismissDialog()
}

/**
 * Settings' "Profiles" section: rename a profile, or remove one (after a
 * confirmation saying what that deletes). Removing the active profile
 * switches to the most recently used of the rest, or back to sign-in when
 * none remain; either way the root rebuilds, so the outcome is said through
 * [appMessages].
 */
class ProfilesSettingsViewModel(
    private val profiles: ProfileManager,
    private val appMessages: Messages = Messages(),
) : ViewModel(), ProfilesSectionActions {
    private val _state = MutableStateFlow(ProfilesSectionState())
    val state: StateFlow<ProfilesSectionState> = _state.asStateFlow()

    init {
        viewModelScope.launch {
            profiles.state.collect { profilesState -> _state.update { it.copy(rows = profileRows(profilesState)) } }
        }
    }

    private fun ready(): ProfilesState.Ready? = profiles.state.value as? ProfilesState.Ready

    override fun startRename(id: ProfileId) {
        val profile = ready()?.profile(id) ?: return
        _state.update { it.copy(rename = RenameDialog(id, profile.label), remove = null) }
    }

    override fun onRenameChange(text: String) {
        _state.update { current -> current.copy(rename = current.rename?.copy(text = text, action = ActionState.Idle)) }
    }

    override fun saveRename() {
        val dialog = _state.value.rename ?: return
        if (!dialog.canSave) return
        _state.update { it.copy(rename = dialog.copy(action = ActionState.Running)) }
        viewModelScope.launch {
            when (val renamed = profiles.rename(dialog.id, dialog.text)) {
                is Outcome.Ok -> _state.update { it.copy(rename = null) }
                is Outcome.Err -> _state.update { it.copy(rename = dialog.copy(action = ActionState.Failed(renamed.error))) }
            }
        }
    }

    override fun askRemove(id: ProfileId) {
        val ready = ready() ?: return
        val profile = ready.profile(id) ?: return
        val active = ready.active == id
        val next = if (active) ready.profiles.firstOrNull { it.id != id } else null
        val dialog = RemoveDialog(id, ProfileText.removeTitle(profile), ProfileText.removeBody(profile, active, next))
        _state.update { it.copy(remove = dialog, rename = null) }
    }

    override fun confirmRemove() {
        val dialog = _state.value.remove ?: return
        if (dialog.action.running) return
        _state.update { it.copy(remove = dialog.copy(action = ActionState.Running)) }
        viewModelScope.launch {
            when (val removed = profiles.remove(dialog.id)) {
                is Outcome.Ok -> {
                    appMessages.send(ProfileText.removed(removed.value))
                    _state.update { it.copy(remove = null) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "profile_remove_failed", "error" to removed.error::class.simpleName)
                    _state.update { it.copy(remove = dialog.copy(action = ActionState.Failed(removed.error))) }
                }
            }
        }
    }

    override fun dismissDialog() {
        val current = _state.value
        if (current.rename?.action?.running == true || current.remove?.action?.running == true) return
        _state.update { it.copy(rename = null, remove = null) }
    }

    private companion object {
        const val TAG = "RostrumProfiles"
    }
}
