package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.profiles.ProfileHandle
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.ui.common.ActionState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SignInUiState(
    /** Why the app signed out on its own, if it did. */
    val notice: String? = null,
    /** The signed-out profile this screen signs in again; `null` on first run. */
    val profileLabel: String? = null,
    val tokenFormOpen: Boolean = false,
    val token: String = "",
    val submit: ActionState = ActionState.Idle,
) {
    val canSubmit: Boolean get() = token.isNotBlank() && submit != ActionState.Running

    override fun toString(): String =
        "SignInUiState(notice=$notice, profileLabel=$profileLabel, tokenFormOpen=$tokenFormOpen, token=redacted, submit=$submit)"
}

interface SignInActions {
    fun toggleTokenForm()
    fun onTokenChange(token: String)
    fun signIn()
}

object NoSignInActions : SignInActions {
    override fun toggleTokenForm() = Unit
    override fun onTokenChange(token: String) = Unit
    override fun signIn() = Unit
}

/**
 * The personal-access-token path of the sign-in screen. On first run
 * ([profile] `null`) the token makes a new profile, which becomes active;
 * for a profile that lost its token it signs that profile in again. Success
 * needs no navigation: the root rebuilds the graph for the signed-in profile.
 */
class SignInViewModel(
    private val profiles: ProfileManager,
    private val profile: ProfileHandle?,
) : ViewModel(), SignInActions {
    private val _state = MutableStateFlow(SignInUiState())
    val state: StateFlow<SignInUiState> = _state.asStateFlow()

    init {
        if (profile != null) {
            viewModelScope.launch {
                profile.session.state.collect { sessionState ->
                    val notice = ((sessionState as? SessionState.Ready)?.github as? GitHubAuth.SignedOut)?.notice
                    _state.update { it.copy(notice = notice) }
                }
            }
            viewModelScope.launch {
                profiles.state.collect { profilesState ->
                    val label = (profilesState as? ProfilesState.Ready)?.profile(profile.id)?.label
                    _state.update { it.copy(profileLabel = label) }
                }
            }
        }
    }

    override fun toggleTokenForm() {
        _state.update { it.copy(tokenFormOpen = !it.tokenFormOpen) }
    }

    override fun onTokenChange(token: String) {
        _state.update { it.copy(token = token, submit = ActionState.Idle) }
    }

    override fun signIn() {
        val current = _state.value
        if (!current.canSubmit) return
        _state.update { it.copy(submit = ActionState.Running) }
        viewModelScope.launch {
            val result = if (profile == null) createProfile(current.token) else profile.session.signInWithToken(current.token)
            when (result) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "token_sign_in", "new_profile" to (profile == null))
                    _state.update { it.copy(token = "", submit = ActionState.Idle) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "token_sign_in_failed", "error" to result.error::class.simpleName)
                    _state.update { it.copy(submit = ActionState.Failed(result.error)) }
                }
            }
        }
    }

    private suspend fun createProfile(token: String): Outcome<Unit> =
        when (val created = profiles.createTokenProfile(token, label = null)) {
            is Outcome.Err -> created
            is Outcome.Ok -> when (val switched = profiles.switchTo(created.value.id)) {
                is Outcome.Err -> switched
                is Outcome.Ok -> Outcome.Ok(Unit)
            }
        }

    private companion object {
        const val TAG = "RostrumSignIn"
    }
}
