package io.github.rhizonymph.rostrum.ui.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.valueOrNull
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the Settings screen can ask for. The ViewModel implements it; previews use [NoSettingsActions]. */
interface SettingsActions {
    fun retry()
    fun onAddInputChange(input: String)
    fun addRepo()
    fun removeRepo(repo: String)
    fun setRefreshInterval(seconds: Long)
    fun setNotifyNewPullRequests(enabled: Boolean)
    fun setNotifyReviewRequests(enabled: Boolean)
    fun signOut()
    fun refreshTokenFromDesktop()
}

object NoSettingsActions : SettingsActions {
    override fun retry() = Unit
    override fun onAddInputChange(input: String) = Unit
    override fun addRepo() = Unit
    override fun removeRepo(repo: String) = Unit
    override fun setRefreshInterval(seconds: Long) = Unit
    override fun setNotifyNewPullRequests(enabled: Boolean) = Unit
    override fun setNotifyReviewRequests(enabled: Boolean) = Unit
    override fun signOut() = Unit
    override fun refreshTokenFromDesktop() = Unit
}

/**
 * The account, the watched repositories, the desktop row and the sync
 * settings. Reloads whenever the session's desktop or sign-in changes, so
 * unpairing from the Desktop tab shows here at once.
 *
 * @param onNotificationSettingsChanged keeps the background check's schedule
 *   in step after a notification toggle is saved.
 */
class SettingsViewModel(
    private val backend: RostrumBackend,
    private val session: SessionRepository,
    private val onNotificationSettingsChanged: (Settings) -> Unit,
) : ViewModel(), SettingsActions {
    private val _state = MutableStateFlow(SettingsUiState())
    val state: StateFlow<SettingsUiState> = _state.asStateFlow()
    val messages = Messages()

    private var loadJob: Job? = null

    init {
        viewModelScope.launch {
            session.state
                .map { it.sessionKey() }
                .distinctUntilChanged()
                .collect { reload() }
        }
    }

    private fun SessionState.sessionKey(): Pair<DesktopLink?, Boolean> {
        val ready = this as? SessionState.Ready
        return ready?.desktop to (ready?.github is GitHubAuth.SignedIn)
    }

    override fun retry() {
        _state.update { it.copy(content = UiState.Loading) }
        reload()
    }

    /** Reload without the loading state, e.g. after the desktop's settings were copied. */
    fun refreshContent() = reload()

    private fun reload() {
        loadJob?.cancel()
        loadJob = viewModelScope.launch { load() }
    }

    private suspend fun load() {
        val settings = when (val outcome = backend.settings().logErr(TAG, "settings_load_failed")) {
            is Outcome.Err -> {
                if (_state.value.content is UiState.Loaded) {
                    messages.send(outcome.error.describe())
                } else {
                    _state.update { it.copy(content = UiState.Error(outcome.error)) }
                }
                return
            }
            is Outcome.Ok -> outcome.value
        }
        val ready = session.state.value as? SessionState.Ready
        val host = RostrumBackend.GITHUB_COM
        val viewer = when (val outcome = backend.viewer()) {
            is Outcome.Ok -> AccountViewer.Known(outcome.value.login)
            is Outcome.Err -> AccountViewer.Unknown(outcome.error)
        }
        val desktop = if (ready?.desktop is DesktopLink.Paired) {
            when (val machine = backend.machineInfo()) {
                is Outcome.Ok -> DesktopSummary.Connected(machine.value)
                is Outcome.Err -> DesktopSummary.Unreachable(machine.error)
            }
        } else {
            DesktopSummary.NotPaired
        }
        val feed = backend.cachedFeed().valueOrNull()
        val machine = (desktop as? DesktopSummary.Connected)?.machine
        _state.update {
            it.copy(
                content = UiState.Loaded(
                    SettingsContent(
                        account = AccountInfo(viewer, host),
                        repos = repoRows(settings.repos, feed, machine),
                        desktop = desktop,
                        refreshIntervalSecs = settings.refreshIntervalSecs,
                        notifyNewPullRequests = settings.notifyNewPullRequests,
                        notifyReviewRequests = settings.notifyReviewRequests,
                    ),
                ),
            )
        }
    }

    override fun onAddInputChange(input: String) {
        _state.update { it.copy(addRepo = it.addRepo.copy(input = input, error = null)) }
    }

    override fun addRepo() {
        val input = _state.value.addRepo.input
        if (input.isBlank() || _state.value.addRepo.running) return
        _state.update { it.copy(addRepo = it.addRepo.copy(running = true, error = null)) }
        viewModelScope.launch {
            when (val added = backend.addRepo(input)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "repo_added", "repo" to added.value)
                    _state.update { it.copy(addRepo = AddRepoState()) }
                    messages.send("Added ${added.value}")
                    backend.refreshRepo(added.value).logErr(TAG, "repo_refresh_failed", "repo" to added.value)
                    load()
                }
                is Outcome.Err -> {
                    val error = added.error
                    _state.update {
                        it.copy(addRepo = it.addRepo.copy(running = false, error = error.takeIf { e -> e.isRepoInputError }))
                    }
                    if (!error.isRepoInputError) {
                        RostrumLog.w(TAG, "repo_add_failed", "error" to error::class.simpleName)
                        messages.send(error.describe())
                    }
                }
            }
        }
    }

    override fun removeRepo(repo: String) {
        if (repo in _state.value.removing) return
        _state.update { it.copy(removing = it.removing + repo) }
        viewModelScope.launch {
            when (val removed = backend.removeRepo(repo)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "repo_removed", "repo" to repo, "was_watched" to removed.value)
                    messages.send("Removed $repo")
                    load()
                }
                is Outcome.Err -> messages.send(removed.error.describe())
            }
            _state.update { it.copy(removing = it.removing - repo) }
        }
    }

    override fun setRefreshInterval(seconds: Long) {
        viewModelScope.launch {
            applySettings(backend.setRefreshInterval(seconds), notify = false)
        }
    }

    override fun setNotifyNewPullRequests(enabled: Boolean) {
        val content = loaded() ?: return
        viewModelScope.launch {
            applySettings(backend.setNotifications(enabled, content.notifyReviewRequests), notify = true)
        }
    }

    override fun setNotifyReviewRequests(enabled: Boolean) {
        val content = loaded() ?: return
        viewModelScope.launch {
            applySettings(backend.setNotifications(content.notifyNewPullRequests, enabled), notify = true)
        }
    }

    private fun applySettings(outcome: Outcome<Settings>, notify: Boolean) {
        when (outcome) {
            is Outcome.Err -> {
                RostrumLog.w(TAG, "settings_save_failed", "error" to outcome.error::class.simpleName)
                messages.send(outcome.error.describe())
            }
            is Outcome.Ok -> {
                val settings = outcome.value
                _state.update { state ->
                    val content = (state.content as? UiState.Loaded)?.data ?: return@update state
                    state.copy(
                        content = UiState.Loaded(
                            content.copy(
                                refreshIntervalSecs = settings.refreshIntervalSecs,
                                notifyNewPullRequests = settings.notifyNewPullRequests,
                                notifyReviewRequests = settings.notifyReviewRequests,
                            ),
                        ),
                    )
                }
                if (notify) {
                    RostrumLog.i(
                        TAG, "notification_settings_saved",
                        "new_pull_requests" to settings.notifyNewPullRequests,
                        "review_requests" to settings.notifyReviewRequests,
                    )
                    onNotificationSettingsChanged(settings)
                }
            }
        }
    }

    override fun signOut() {
        viewModelScope.launch { session.signOut() }
    }

    override fun refreshTokenFromDesktop() {
        val machine = ((loaded()?.desktop) as? DesktopSummary.Connected)?.machine?.name ?: "the desktop"
        viewModelScope.launch {
            when (val result = session.refreshTokenFromDesktop()) {
                is Outcome.Ok -> {
                    messages.send("Signed in with $machine's GitHub token")
                    load()
                }
                is Outcome.Err -> messages.send(result.error.describe())
            }
        }
    }

    private fun loaded(): SettingsContent? = (_state.value.content as? UiState.Loaded)?.data

    private companion object {
        const val TAG = "RostrumSettings"
    }
}
