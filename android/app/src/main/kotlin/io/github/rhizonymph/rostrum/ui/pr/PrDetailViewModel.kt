package io.github.rhizonymph.rostrum.ui.pr

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeFormState
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Clock

/**
 * One pull request: its detail (header, conversation, threads, checks) and
 * every GitHub-side action on it. The desktop half lives in
 * [io.github.rhizonymph.rostrum.ui.pr.branch.BranchViewModel].
 */
class PrDetailViewModel(
    private val pr: PrRef,
    private val backend: RostrumBackend,
    private val clock: Clock,
) : ViewModel() {
    private val _state = MutableStateFlow(PrDetailUiState(pr))
    val state: StateFlow<PrDetailUiState> = _state.asStateFlow()
    val messages = Messages()

    private var loadJob: Job? = null

    private val detail: PullDetail? get() = _state.value.detail.dataOrNull()

    init {
        loadJob = viewModelScope.launch {
            val cached = backend.cachedPullDetail(pr)
            if (cached is Outcome.Ok && cached.value != null) {
                _state.update { it.copy(detail = UiState.Loaded(cached.value), refreshing = true) }
            }
            reload()
        }
    }

    /** Fetch again; on failure keep what is shown and say why. */
    fun refresh() {
        loadJob?.cancel()
        _state.update { current ->
            if (current.detail is UiState.Error) current.copy(detail = UiState.Loading) else current.copy(refreshing = true)
        }
        loadJob = viewModelScope.launch { reload() }
    }

    private suspend fun reload(): Outcome<PullDetail> {
        _state.update { it.copy(refreshing = true) }
        val result = backend.pullDetail(pr)
        when (result) {
            is Outcome.Ok -> _state.update {
                it.copy(detail = UiState.Loaded(result.value), refreshing = false, loadedAt = clock.instant())
            }
            is Outcome.Err -> {
                RostrumLog.w(TAG, "pr_detail_failed", "pr" to pr, "error" to result.error::class.simpleName)
                val hadData = detail != null
                _state.update {
                    if (hadData) it.copy(refreshing = false) else it.copy(detail = UiState.Error(result.error), refreshing = false)
                }
                if (hadData) messages.send(result.error.describe())
            }
        }
        return result
    }

    // --- comments --------------------------------------------------------------

    fun setComment(text: String) = _state.update { it.copy(comment = text) }

    fun postComment() {
        val body = _state.value.comment
        if (body.isBlank() || _state.value.postingComment) return
        _state.update { it.copy(postingComment = true) }
        viewModelScope.launch {
            when (val posted = backend.addComment(pr, body.trim())) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "comment_posted", "pr" to pr)
                    _state.update { it.copy(comment = "", postingComment = false) }
                    reload()
                }
                is Outcome.Err -> {
                    _state.update { it.copy(postingComment = false) }
                    fail("comment_failed", posted.error)
                }
            }
        }
    }

    fun startReply(threadId: String) = _state.update { it.copy(reply = ReplyDraft(threadId)) }

    fun setReplyText(text: String) = _state.update { state -> state.copy(reply = state.reply?.copy(text = text)) }

    fun cancelReply() = _state.update { it.copy(reply = null) }

    fun sendReply() {
        val reply = _state.value.reply ?: return
        if (reply.text.isBlank() || reply.sending) return
        _state.update { it.copy(reply = reply.copy(sending = true)) }
        viewModelScope.launch {
            when (val sent = backend.replyToThread(pr, reply.threadId, reply.text.trim())) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "reply_posted", "pr" to pr, "thread" to reply.threadId)
                    _state.update { it.copy(reply = null) }
                    reload()
                }
                is Outcome.Err -> {
                    _state.update { state -> state.copy(reply = state.reply?.copy(sending = false)) }
                    fail("reply_failed", sent.error)
                }
            }
        }
    }

    // --- labels ----------------------------------------------------------------

    fun openLabels() {
        _state.update { it.copy(labels = LabelPickerState(UiState.Loading)) }
        viewModelScope.launch {
            val available = when (val labels = backend.repositoryLabels(pr.repo)) {
                is Outcome.Ok -> UiState.Loaded(labels.value)
                is Outcome.Err -> UiState.Error(labels.error)
            }
            _state.update { state -> state.copy(labels = state.labels?.copy(available = available)) }
        }
    }

    fun closeLabels() = _state.update { it.copy(labels = null) }

    /** Apply [name] if the pull request lacks it, remove it otherwise. */
    fun toggleLabel(name: String) {
        val applied = detail?.header?.labels?.any { it.name == name } ?: return
        if (_state.value.labels?.pending != null) return
        _state.update { state -> state.copy(labels = state.labels?.copy(pending = name)) }
        viewModelScope.launch {
            val result = if (applied) backend.removeLabel(pr, name) else backend.addLabel(pr, name)
            if (result is Outcome.Ok) {
                RostrumLog.i(TAG, if (applied) "label_removed" else "label_added", "pr" to pr, "label" to name)
                reload()
            } else if (result is Outcome.Err) {
                fail("label_failed", result.error)
            }
            _state.update { state -> state.copy(labels = state.labels?.copy(pending = null)) }
        }
    }

    // --- pull request actions --------------------------------------------------

    /** Move into or out of draft, as the header's action says. Reversible: no confirmation. */
    fun toggleDraft() {
        val action = detail?.header?.draftAction ?: return
        runAction(PrBusy.Draft, if (action.toDraft) "Converted to draft" else "Marked ready for review") {
            backend.setDraft(pr, action.toDraft)
        }
    }

    /** Close without merging. The UI confirms first. */
    fun close() = runAction(PrBusy.Close, "Closed #${pr.number}") { backend.closePullRequest(pr) }

    /** Reopen. The UI confirms first. */
    fun reopen() = runAction(PrBusy.Reopen, "Reopened #${pr.number}") { backend.reopenPullRequest(pr) }

    /** Bring the branch up to date with its base on GitHub, guarded by the shown head. */
    fun updateBranch(method: BranchUpdateMethod) {
        val header = detail?.header ?: return
        val busy = if (method == BranchUpdateMethod.Merge) PrBusy.UpdateMerge else PrBusy.UpdateRebase
        val done = when (method) {
            BranchUpdateMethod.Merge -> "Merged ${header.baseRef} into ${header.headRef}"
            BranchUpdateMethod.Rebase -> "Rebased ${header.headRef} onto ${header.baseRef}"
        }
        runAction(busy, done) { backend.updateBranch(pr, method, header.headSha) }
    }

    private fun runAction(busy: PrBusy, done: String, call: suspend () -> Outcome<Unit>) {
        if (_state.value.busy != null) return
        _state.update { it.copy(busy = busy) }
        viewModelScope.launch {
            when (val result = call()) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "pr_action", "pr" to pr, "action" to busy)
                    reload()
                    messages.send(done)
                }
                is Outcome.Err -> fail("pr_action_failed", result.error, "action" to busy)
            }
            _state.update { it.copy(busy = null) }
        }
    }

    // --- merge -----------------------------------------------------------------

    /** Open the merge sheet; only when the verdict allows merging. */
    fun openMerge() {
        val header = detail?.header ?: return
        if (header.merge.blocksMerge) return
        _state.update { it.copy(merge = MergeFormState.forMethod(MergeMethod.Merge, header)) }
    }

    fun setMergeMethod(method: MergeMethod) {
        val header = detail?.header ?: return
        _state.update { state ->
            val form = state.merge ?: return@update state
            if (form.method == method) state else state.copy(merge = MergeFormState.forMethod(method, header))
        }
    }

    fun setMergeTitle(title: String) = _state.update { state -> state.copy(merge = state.merge?.copy(title = title)) }

    fun setMergeMessage(message: String) = _state.update { state -> state.copy(merge = state.merge?.copy(message = message)) }

    fun dismissMerge() = _state.update { state -> if (state.merge?.submitting == true) state else state.copy(merge = null) }

    /** The sheet is the confirmation: this merges. */
    fun confirmMerge() {
        val form = _state.value.merge ?: return
        val header = detail?.header ?: return
        if (form.submitting) return
        _state.update { it.copy(merge = form.copy(submitting = true, error = null)) }
        viewModelScope.launch {
            val title = form.title.takeIf { form.hasCommitText && it.isNotBlank() }
            val message = form.message.takeIf { form.hasCommitText && it.isNotBlank() }
            when (val merged = backend.merge(pr, form.method, title, message, header.headSha)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "merged", "pr" to pr, "method" to form.method)
                    _state.update { it.copy(merge = null) }
                    reload()
                    messages.send("Merged #${pr.number}")
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "merge_refused", "pr" to pr, "error" to merged.error::class.simpleName)
                    _state.update { state -> state.copy(merge = state.merge?.copy(submitting = false, error = merged.error)) }
                    if (merged.error is BackendError.MergeBlocked) reload()
                }
            }
        }
    }

    private fun fail(event: String, error: BackendError, vararg fields: Pair<String, Any?>) {
        RostrumLog.w(TAG, event, "pr" to pr, *fields, "error" to error::class.simpleName)
        messages.send(error.describe())
    }

    private companion object {
        const val TAG = "RostrumPr"
    }
}
