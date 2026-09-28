package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionRepository
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
    val tokenFormOpen: Boolean = false,
    val token: String = "",
    val host: String = RostrumBackend.GITHUB_COM,
    val submit: ActionState = ActionState.Idle,
) {
    val canSubmit: Boolean get() = token.isNotBlank() && submit != ActionState.Running

    override fun toString(): String =
        "SignInUiState(notice=$notice, tokenFormOpen=$tokenFormOpen, token=redacted, host=$host, submit=$submit)"
}

interface SignInActions {
    fun toggleTokenForm()
    fun onTokenChange(token: String)
    fun onHostChange(host: String)
    fun signIn()
}

object NoSignInActions : SignInActions {
    override fun toggleTokenForm() = Unit
    override fun onTokenChange(token: String) = Unit
    override fun onHostChange(host: String) = Unit
    override fun signIn() = Unit
}

/**
 * The personal-access-token path of the first-run screen. Success needs no
 * navigation: the session flips to signed in and the root rebuilds the graph.
 */
class SignInViewModel(private val session: SessionRepository) : ViewModel(), SignInActions {
    private val _state = MutableStateFlow(SignInUiState())
    val state: StateFlow<SignInUiState> = _state.asStateFlow()

    init {
        viewModelScope.launch {
            session.state.collect { sessionState ->
                val notice = ((sessionState as? SessionState.Ready)?.github as? GitHubAuth.SignedOut)?.notice
                _state.update { it.copy(notice = notice) }
            }
        }
    }

    override fun toggleTokenForm() {
        _state.update { it.copy(tokenFormOpen = !it.tokenFormOpen) }
    }

    override fun onTokenChange(token: String) {
        _state.update { it.copy(token = token, submit = ActionState.Idle) }
    }

    override fun onHostChange(host: String) {
        _state.update { it.copy(host = host, submit = ActionState.Idle) }
    }

    override fun signIn() {
        val current = _state.value
        if (!current.canSubmit) return
        _state.update { it.copy(submit = ActionState.Running) }
        viewModelScope.launch {
            when (val result = session.signInWithToken(current.token, current.host)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "token_sign_in", "login" to result.value.login)
                    _state.update { it.copy(token = "", submit = ActionState.Idle) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "token_sign_in_failed", "error" to result.error::class.simpleName)
                    _state.update { it.copy(submit = ActionState.Failed(result.error)) }
                }
            }
        }
    }

    private companion object {
        const val TAG = "RostrumSignIn"
    }
}
