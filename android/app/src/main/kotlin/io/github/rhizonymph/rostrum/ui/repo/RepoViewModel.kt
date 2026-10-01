package io.github.rhizonymph.rostrum.ui.repo

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.common.toUiState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Clock

/** What the repository screen can ask for. */
interface RepoActions {
    fun selectTab(tab: RepoTab)
    fun refresh()
    fun retryBranches()
    fun openTrunkEditor()
    fun setTrunkDetect(detect: Boolean)
    fun onTrunkTextChange(text: String)
    fun saveTrunks()
    fun closeTrunkEditor()
}

/**
 * One repository: its pull requests (stacks grouped) and issues from the
 * feed's state, unfiltered and in the item sort, followed as the feed
 * changes; its branch tree, fetched when the Branches tab is first shown;
 * and its trunk setting.
 */
class RepoViewModel(
    private val backend: RostrumBackend,
    private val repo: String,
    private val clock: Clock,
) : ViewModel(), RepoActions {
    private val _state = MutableStateFlow(RepoUiState(now = clock.instant()))
    val state: StateFlow<RepoUiState> = _state.asStateFlow()
    val messages = Messages()

    init {
        viewModelScope.launch { loadOverview() }
        viewModelScope.launch { backend.feedUpdates.collect { loadOverview() } }
    }

    /** No network: the core answers from the feed's state. */
    private suspend fun loadOverview() {
        val overview = backend.repoOverview(repo).logErr(TAG, "repo_overview_failed")
        _state.update { state ->
            val keep = overview is Outcome.Err && state.overview is UiState.Loaded
            state.copy(overview = if (keep) state.overview else overview.toUiState(), now = clock.instant())
        }
    }

    private suspend fun loadBranches() {
        val tree = backend.branchTree(repo).logErr(TAG, "branch_tree_failed")
        _state.update { state ->
            val keep = tree is Outcome.Err && state.branches is UiState.Loaded
            if (keep && tree is Outcome.Err) messages.send("Couldn't refresh the branches. ${tree.error.describe()}")
            state.copy(branches = if (keep) state.branches else tree.toUiState())
        }
    }

    override fun selectTab(tab: RepoTab) {
        val first = tab == RepoTab.Branches && _state.value.branches == null
        _state.update { it.copy(tab = tab, branches = if (first) UiState.Loading else it.branches) }
        if (first) viewModelScope.launch { loadBranches() }
    }

    /** Re-fetch this repository from GitHub (and its branches, when shown). */
    override fun refresh() {
        if (_state.value.refreshing) return
        _state.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            when (val refreshed = backend.refreshRepo(repo)) {
                is Outcome.Ok -> RostrumLog.i(TAG, "repo_refreshed", "repo" to repo)
                is Outcome.Err -> messages.send("Couldn't refresh. ${refreshed.error.describe()}")
            }
            loadOverview()
            if (_state.value.branches != null) loadBranches()
            _state.update { it.copy(refreshing = false) }
        }
    }

    override fun retryBranches() {
        _state.update { it.copy(branches = UiState.Loading) }
        viewModelScope.launch { loadBranches() }
    }

    override fun openTrunkEditor() {
        val known = _state.value.branches?.dataOrNull()?.trunks
        if (known != null) {
            _state.update { it.copy(trunkEditor = TrunkEditor.of(known)) }
            return
        }
        viewModelScope.launch {
            when (val trunks = backend.trunks(repo)) {
                is Outcome.Ok -> _state.update { it.copy(trunkEditor = TrunkEditor.of(trunks.value)) }
                is Outcome.Err -> messages.send(trunks.error.describe())
            }
        }
    }

    override fun setTrunkDetect(detect: Boolean) {
        _state.update { state -> state.copy(trunkEditor = state.trunkEditor?.copy(detect = detect, save = ActionState.Idle)) }
    }

    override fun onTrunkTextChange(text: String) {
        _state.update { state -> state.copy(trunkEditor = state.trunkEditor?.copy(text = text, save = ActionState.Idle)) }
    }

    /** Detection, or the typed names (the core validates each). */
    override fun saveTrunks() {
        val editor = _state.value.trunkEditor ?: return
        if (editor.save.running) return
        val names = if (editor.detect) null else parseTrunkNames(editor.text)
        _state.update { it.copy(trunkEditor = editor.copy(save = ActionState.Running)) }
        viewModelScope.launch {
            when (val saved = backend.setTrunks(repo, names)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "trunks_saved", "repo" to repo, "detected" to saved.value.detected)
                    _state.update { it.copy(trunkEditor = null, branches = UiState.Loading) }
                    loadBranches()
                }
                is Outcome.Err -> _state.update { state ->
                    state.copy(trunkEditor = state.trunkEditor?.copy(save = ActionState.Failed(saved.error)))
                }
            }
        }
    }

    override fun closeTrunkEditor() {
        _state.update { it.copy(trunkEditor = null) }
    }

    private companion object {
        const val TAG = "RostrumRepo"
    }
}
