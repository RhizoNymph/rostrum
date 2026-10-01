package io.github.rhizonymph.rostrum.ui.issue

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.common.toUiState
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.PickerOption
import io.github.rhizonymph.rostrum.ui.components.PickerState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Clock

/** What the issue screen can ask for. */
interface IssueActions {
    fun retry()
    fun refresh()
    fun onCommentChange(text: String)
    fun sendComment()
    fun run(action: IssueStateAction)
    fun openPicker(kind: PickerKind)
    fun retryPicker()
    fun toggle(key: String)
    fun closePicker()
}

object NoIssueActions : IssueActions {
    override fun retry() = Unit
    override fun refresh() = Unit
    override fun onCommentChange(text: String) = Unit
    override fun sendComment() = Unit
    override fun run(action: IssueStateAction) = Unit
    override fun openPicker(kind: PickerKind) = Unit
    override fun retryPicker() = Unit
    override fun toggle(key: String) = Unit
    override fun closePicker() = Unit
}

/**
 * One issue: paints the cached screen, then GitHub's; comments, closes as
 * completed or not planned, reopens, and edits labels and assignees through
 * pickers. After every change it re-reads the issue, so the header and the
 * timeline show what GitHub now has.
 */
class IssueViewModel(
    private val backend: RostrumBackend,
    private val issue: IssueRef,
    private val clock: Clock,
) : ViewModel(), IssueActions {
    private val _state = MutableStateFlow(IssueUiState(now = clock.instant()))
    val state: StateFlow<IssueUiState> = _state.asStateFlow()
    val messages = Messages()

    init {
        viewModelScope.launch { load() }
    }

    private suspend fun load() {
        (backend.cachedIssueDetail(issue) as? Outcome.Ok)?.value?.let { cached ->
            _state.update { it.copy(detail = UiState.Loaded(cached), refreshing = true) }
        }
        fetch()
    }

    /** Read the issue from GitHub; a failure keeps what is shown and says so. */
    private suspend fun fetch() {
        val result = backend.issueDetail(issue)
        _state.update { state ->
            val shown = state.detail is UiState.Loaded
            val next = if (result is Outcome.Err && shown) state.detail else result.toUiState()
            state.copy(detail = next, refreshing = false, now = clock.instant())
        }
        if (result is Outcome.Err) {
            RostrumLog.w(TAG, "issue_load_failed", "issue" to issue, "error" to result.error::class.simpleName)
            if (_state.value.detail is UiState.Loaded) messages.send("Couldn't refresh. ${result.error.describe()}")
        }
    }

    override fun retry() {
        _state.update { it.copy(detail = UiState.Loading) }
        viewModelScope.launch { fetch() }
    }

    override fun refresh() {
        if (_state.value.refreshing) return
        _state.update { it.copy(refreshing = true) }
        viewModelScope.launch { fetch() }
    }

    override fun onCommentChange(text: String) {
        _state.update { it.copy(comment = text) }
    }

    override fun sendComment() {
        val text = _state.value.comment
        if (text.isBlank() || _state.value.sending) return
        _state.update { it.copy(sending = true) }
        viewModelScope.launch {
            when (val sent = backend.commentOnIssue(issue, text)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "issue_commented", "issue" to issue)
                    _state.update { it.copy(comment = "", sending = false) }
                    fetch()
                }
                is Outcome.Err -> {
                    failed("issue_comment", sent.error)
                    _state.update { it.copy(sending = false) }
                }
            }
        }
    }

    override fun run(action: IssueStateAction) {
        if (_state.value.stateAction.running) return
        _state.update { it.copy(stateAction = ActionState.Running) }
        viewModelScope.launch {
            val result = when (action) {
                is IssueStateAction.Close -> backend.closeIssue(issue, action.reason)
                IssueStateAction.Reopen -> backend.reopenIssue(issue)
            }
            when (result) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "issue_state_changed", "issue" to issue, "action" to action)
                    messages.send(actionDone(action, issue.number))
                    _state.update { it.copy(stateAction = ActionState.Idle) }
                    fetch()
                }
                is Outcome.Err -> {
                    failed("issue_state", result.error)
                    _state.update { it.copy(stateAction = ActionState.Failed(result.error)) }
                }
            }
        }
    }

    // --- pickers -------------------------------------------------------------------

    override fun openPicker(kind: PickerKind) {
        _state.update { it.copy(picker = PickerState(kind, UiState.Loading)) }
        viewModelScope.launch { loadOptions(kind) }
    }

    override fun retryPicker() {
        _state.value.picker?.let { openPicker(it.kind) }
    }

    private suspend fun loadOptions(kind: PickerKind) {
        val options: Outcome<List<PickerOption>> = when (kind) {
            PickerKind.Labels -> when (val labels = backend.repositoryLabels(issue.repo)) {
                is Outcome.Ok -> Outcome.Ok(labels.value.map { PickerOption.Label(it) })
                is Outcome.Err -> labels
            }
            PickerKind.Assignees -> when (val users = backend.assignableUsers(issue.repo)) {
                is Outcome.Ok -> Outcome.Ok(users.value.map { PickerOption.Person(it) })
                is Outcome.Err -> users
            }
        }
        _state.update { state ->
            val picker = state.picker?.takeIf { it.kind == kind } ?: return@update state
            state.copy(picker = picker.copy(options = options.toUiState()))
        }
    }

    /** Add or remove a label or assignee, one at a time. */
    override fun toggle(key: String) {
        val state = _state.value
        val picker = state.picker ?: return
        if (picker.pending != null) return
        val adding = key !in state.pickerSelection
        _state.update { it.copy(picker = picker.copy(pending = key)) }
        viewModelScope.launch {
            val result = when (picker.kind) {
                PickerKind.Labels -> if (adding) backend.addIssueLabel(issue, key) else backend.removeIssueLabel(issue, key)
                PickerKind.Assignees -> if (adding) backend.addIssueAssignee(issue, key) else backend.removeIssueAssignee(issue, key)
            }
            if (result is Outcome.Err) failed("issue_${picker.kind.name.lowercase()}", result.error)
            fetch()
            _state.update { current -> current.copy(picker = current.picker?.copy(pending = null)) }
        }
    }

    override fun closePicker() {
        _state.update { it.copy(picker = null) }
    }

    private fun failed(event: String, error: BackendError) {
        RostrumLog.w(TAG, "${event}_failed", "issue" to issue, "error" to error::class.simpleName)
        messages.send(error.describe())
    }

    private companion object {
        const val TAG = "RostrumIssue"
    }
}
