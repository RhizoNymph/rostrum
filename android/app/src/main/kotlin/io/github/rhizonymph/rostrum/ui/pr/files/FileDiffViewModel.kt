package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.ChangedFile
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.common.toUiState
import io.github.rhizonymph.rostrum.ui.review.CommentComposer
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * One file's diff with previous/next through the ranked files, line and
 * range selection feeding the [composer], inline replies, and the pending
 * review's drafts shown at their lines.
 */
class FileDiffViewModel(
    private val backend: RostrumBackend,
    private val pr: PrRef,
    initialFileIndex: Int,
) : ViewModel() {
    private val _state = MutableStateFlow(FileDiffState(fileIndex = initialFileIndex))
    val state: StateFlow<FileDiffState> = _state.asStateFlow()

    val messages = Messages()

    val composer = CommentComposer(
        backend = backend,
        pr = pr,
        scope = viewModelScope,
        messages = messages,
        onPendingChanged = { pending ->
            _state.update { it.copy(pending = pending) }
            reloadDiff()
        },
        onPosted = { reloadDiff() },
        onClosed = { clearSelection() },
    )

    private var files: List<ChangedFile> = emptyList()
    private var diffJob: Job? = null

    init {
        loadOverview()
        loadPending()
        loadDiff(initialFileIndex, showLoading = true)
    }

    fun retry() {
        if (_state.value.nav == null) loadOverview()
        loadPending()
        loadDiff(_state.value.fileIndex, showLoading = true)
    }

    // --- navigation ----------------------------------------------------------------

    fun previousFile() {
        _state.value.nav?.previous?.let(::openFile)
    }

    fun nextFile() {
        _state.value.nav?.next?.let(::openFile)
    }

    private fun openFile(index: Int) {
        composer.dismiss()
        _state.update { state ->
            state.copy(
                fileIndex = index,
                nav = state.nav?.let { FileNav.of(it.order, index) },
                file = files.getOrNull(index),
                selection = null,
                selectedRows = emptySet(),
                reply = null,
            )
        }
        loadDiff(index, showLoading = true)
    }

    fun toggleSoftWrap() = _state.update { it.copy(softWrap = !it.softWrap) }

    fun toggleViewed() = _state.update {
        it.copy(viewed = if (it.isViewed) it.viewed - it.fileIndex else it.viewed + it.fileIndex)
    }

    // --- selection and comments ------------------------------------------------------

    /** Tap on a line number: comment on that one line. */
    fun onLineNumberTap(row: Int) {
        val rows = _state.value.rows
        val selection = DiffSelection.start(rows, row) ?: return
        _state.update { it.copy(selection = selection, selectedRows = DiffSelection.selectedRows(selection, rows)) }
        openComposer()
    }

    /** Long press on a line number: start a range there. */
    fun onSelectionStart(row: Int) {
        val rows = _state.value.rows
        val selection = DiffSelection.start(rows, row) ?: return
        _state.update { it.copy(selection = selection, selectedRows = DiffSelection.selectedRows(selection, rows)) }
    }

    /** The drag reached [row]; the range follows within its side and hunk. */
    fun onSelectionMove(row: Int) {
        val current = _state.value.selection ?: return
        val rows = _state.value.rows
        val moved = DiffSelection.extend(current, rows, row)
        if (moved != current) {
            _state.update { it.copy(selection = moved, selectedRows = DiffSelection.selectedRows(moved, rows)) }
        }
    }

    /** The drag ended: comment on the range. */
    fun onSelectionEnd() {
        if (_state.value.selection != null) openComposer()
    }

    fun clearSelection() = _state.update { it.copy(selection = null, selectedRows = emptySet()) }

    private fun openComposer() {
        val state = _state.value
        val target = state.selection?.let { DiffSelection.target(it, state.rows) } ?: return clearSelection()
        composer.openNew(target, state.pending ?: emptyPending())
        RostrumLog.d(TAG, "comment_started", "pr" to pr, "path" to target.path, "line" to target.line, "range" to (target.rangeStart != null))
    }

    private fun emptyPending() = PendingReview(
        repo = pr.repo,
        number = pr.number,
        drafts = emptyList(),
        draftedAgainst = null,
        headSha = _state.value.diff.dataOrNull()?.headSha.orEmpty(),
        stale = false,
    )

    fun editDraft(draftId: Long) {
        val pending = _state.value.pending ?: return
        val draft = pending.drafts.firstOrNull { it.id == draftId } ?: return
        composer.openEdit(draft, pending)
    }

    fun deleteDraft(draftId: Long) {
        viewModelScope.launch {
            when (val removed = backend.removeDraft(pr, draftId).logErr(TAG, "draft_remove_failed", "pr" to pr)) {
                is Outcome.Err -> messages.send(removed.error.describe())
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "draft_removed", "pr" to pr, "draft" to draftId)
                    _state.update { it.copy(pending = removed.value) }
                    messages.send("Pending comment deleted")
                    reloadDiff()
                }
            }
        }
    }

    // --- replies ---------------------------------------------------------------------

    fun startReply(threadId: String) = _state.update { it.copy(reply = ThreadReply(threadId)) }

    fun setReplyText(text: String) = _state.update { state ->
        state.copy(reply = state.reply?.copy(text = text, action = ActionState.Idle))
    }

    fun cancelReply() = _state.update { it.copy(reply = null) }

    fun sendReply() {
        val reply = _state.value.reply ?: return
        if (reply.text.isBlank() || reply.action.running) return
        _state.update { it.copy(reply = reply.copy(action = ActionState.Running)) }
        viewModelScope.launch {
            when (val sent = backend.replyToThread(pr, reply.threadId, reply.text.trim()).logErr(TAG, "reply_failed", "pr" to pr)) {
                is Outcome.Err -> _state.update { state ->
                    state.copy(reply = state.reply?.takeIf { it.threadId == reply.threadId }?.copy(action = ActionState.Failed(sent.error)))
                }
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "reply_posted", "pr" to pr, "thread" to reply.threadId)
                    _state.update { it.copy(reply = null) }
                    messages.send("Reply posted")
                    reloadDiff()
                }
            }
        }
    }

    // --- review ----------------------------------------------------------------------

    /** A review went out from the submit sheet: drafts became threads. */
    fun onReviewSubmitted() {
        loadPending()
        reloadDiff()
    }

    /** The submit sheet closed; drafts may have been edited there. */
    fun refreshPending() {
        loadPending()
        reloadDiff()
    }

    // --- loading ---------------------------------------------------------------------

    private fun loadOverview() {
        viewModelScope.launch {
            when (val overview = backend.filesOverview(pr).logErr(TAG, "diff_overview_failed", "pr" to pr)) {
                is Outcome.Err -> Unit
                is Outcome.Ok -> {
                    files = overview.value.files
                    val order = overview.value.ranked.map { it.fileIndex }
                    _state.update { state ->
                        state.copy(nav = FileNav.of(order, state.fileIndex), file = files.getOrNull(state.fileIndex) ?: state.file)
                    }
                }
            }
        }
    }

    private fun loadPending() {
        viewModelScope.launch {
            when (val pending = backend.pendingReview(pr).logErr(TAG, "pending_failed", "pr" to pr)) {
                is Outcome.Err -> Unit
                is Outcome.Ok -> _state.update { it.copy(pending = pending.value) }
            }
        }
    }

    private fun loadDiff(index: Int, showLoading: Boolean) {
        diffJob?.cancel()
        if (showLoading) _state.update { it.copy(diff = UiState.Loading) }
        diffJob = viewModelScope.launch {
            val outcome = backend.fileDiff(pr, index).logErr(TAG, "file_diff_failed", "pr" to pr, "file" to index)
            if (_state.value.fileIndex != index) return@launch
            if (!showLoading && outcome is Outcome.Err) {
                // A failed background refresh keeps the rows already shown.
                messages.send(outcome.error.describe())
                return@launch
            }
            val diff = outcome.toUiState()
            _state.update { state ->
                if (state.fileIndex != index) state else state.copy(diff = diff, file = state.file ?: diff.dataOrNull()?.file)
            }
        }
    }

    /** Refresh the open file's rows (drafts, threads) without blanking the screen. */
    private fun reloadDiff() = loadDiff(_state.value.fileIndex, showLoading = false)

    private companion object {
        const val TAG = "RostrumDiff"
    }
}
