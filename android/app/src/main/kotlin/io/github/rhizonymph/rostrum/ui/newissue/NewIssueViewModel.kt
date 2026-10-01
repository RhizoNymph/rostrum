package io.github.rhizonymph.rostrum.ui.newissue

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.toUiState
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.PickerOption
import io.github.rhizonymph.rostrum.ui.components.PickerState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the new-issue form can ask for. */
interface NewIssueActions {
    fun openRepoPicker()
    fun closeRepoPicker()
    fun chooseRepo(repo: String)
    fun onTitleChange(title: String)
    fun onBodyChange(body: String)
    fun showMode(mode: BodyMode)
    fun openPicker(kind: PickerKind)
    fun retryPicker()
    fun toggle(key: String)
    fun closePicker()
    fun submit()
}

object NoNewIssueActions : NewIssueActions {
    override fun openRepoPicker() = Unit
    override fun closeRepoPicker() = Unit
    override fun chooseRepo(repo: String) = Unit
    override fun onTitleChange(title: String) = Unit
    override fun onBodyChange(body: String) = Unit
    override fun showMode(mode: BodyMode) = Unit
    override fun openPicker(kind: PickerKind) = Unit
    override fun retryPicker() = Unit
    override fun toggle(key: String) = Unit
    override fun closePicker() = Unit
    override fun submit() = Unit
}

/**
 * Opening an issue: pick a watched repository (preset from its screen), a
 * title, a markdown body (Preview renders it with the core), and labels
 * and assignees from that repository. Changing the repository drops the
 * labels and assignees, which belong to the old one.
 */
class NewIssueViewModel(
    private val backend: RostrumBackend,
    presetRepo: String?,
    private val appMessages: Messages = Messages(),
) : ViewModel(), NewIssueActions {
    private val _state = MutableStateFlow(NewIssueUiState(repo = presetRepo))
    val state: StateFlow<NewIssueUiState> = _state.asStateFlow()
    val messages = Messages()

    init {
        viewModelScope.launch {
            val repos = when (val settings = backend.settings()) {
                is Outcome.Ok -> Outcome.Ok(settings.value.repos)
                is Outcome.Err -> settings
            }
            _state.update { state ->
                val list = (repos as? Outcome.Ok)?.value.orEmpty()
                state.copy(repos = repos.toUiState(), repo = state.repo ?: list.singleOrNull())
            }
        }
    }

    override fun openRepoPicker() {
        _state.update { it.copy(repoPickerOpen = true) }
    }

    override fun closeRepoPicker() {
        _state.update { it.copy(repoPickerOpen = false) }
    }

    override fun chooseRepo(repo: String) {
        _state.update { state ->
            if (state.repo == repo) {
                state.copy(repoPickerOpen = false)
            } else {
                state.copy(repo = repo, labels = emptySet(), assignees = emptySet(), repoPickerOpen = false)
            }
        }
    }

    override fun onTitleChange(title: String) {
        _state.update { it.copy(title = title, submit = ActionState.Idle) }
    }

    override fun onBodyChange(body: String) {
        _state.update { it.copy(body = body) }
    }

    override fun showMode(mode: BodyMode) {
        val state = _state.value
        if (mode == BodyMode.Preview) {
            val blocks = when (val rendered = backend.renderMarkdown(state.body, state.repo.orEmpty())) {
                is Outcome.Ok -> rendered.value
                is Outcome.Err -> {
                    messages.send("Couldn't render the preview. ${rendered.error.describe()}")
                    return
                }
            }
            _state.update { it.copy(mode = mode, preview = blocks) }
        } else {
            _state.update { it.copy(mode = mode) }
        }
    }

    override fun openPicker(kind: PickerKind) {
        val repo = _state.value.repo ?: return
        _state.update { it.copy(picker = PickerState(kind, UiState.Loading)) }
        viewModelScope.launch {
            val options: Outcome<List<PickerOption>> = when (kind) {
                PickerKind.Labels -> when (val labels = backend.repositoryLabels(repo)) {
                    is Outcome.Ok -> Outcome.Ok(labels.value.map { PickerOption.Label(it) })
                    is Outcome.Err -> labels
                }
                PickerKind.Assignees -> when (val users = backend.assignableUsers(repo)) {
                    is Outcome.Ok -> Outcome.Ok(users.value.map { PickerOption.Person(it) })
                    is Outcome.Err -> users
                }
            }
            _state.update { state ->
                val picker = state.picker?.takeIf { it.kind == kind } ?: return@update state
                state.copy(picker = picker.copy(options = options.toUiState()))
            }
        }
    }

    override fun retryPicker() {
        _state.value.picker?.let { openPicker(it.kind) }
    }

    /** The form keeps its own choices; nothing reaches GitHub until submit. */
    override fun toggle(key: String) {
        _state.update { state ->
            when (state.picker?.kind) {
                PickerKind.Labels -> state.copy(labels = if (key in state.labels) state.labels - key else state.labels + key)
                PickerKind.Assignees -> state.copy(assignees = if (key in state.assignees) state.assignees - key else state.assignees + key)
                null -> state
            }
        }
    }

    override fun closePicker() {
        _state.update { it.copy(picker = null) }
    }

    override fun submit() {
        val state = _state.value
        val repo = state.repo ?: return
        if (!state.canSubmit) return
        _state.update { it.copy(submit = ActionState.Running) }
        viewModelScope.launch {
            when (val created = backend.createIssue(repo, state.title.trim(), state.body, state.labels.toList(), state.assignees.toList())) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "issue_created", "repo" to repo, "number" to created.value)
                    appMessages.send("Opened #${created.value} in $repo")
                    _state.update { it.copy(submit = ActionState.Idle, created = IssueRef(repo, created.value)) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "issue_create_failed", "repo" to repo, "error" to created.error::class.simpleName)
                    _state.update { it.copy(submit = ActionState.Failed(created.error)) }
                }
            }
        }
    }

    private companion object {
        const val TAG = "RostrumNewIssue"
    }
}
