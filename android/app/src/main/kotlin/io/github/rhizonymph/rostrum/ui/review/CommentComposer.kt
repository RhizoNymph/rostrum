package io.github.rhizonymph.rostrum.ui.review

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.running
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Whether the sheet writes a new inline comment or changes a pending one. */
sealed interface ComposerMode {
    data class New(val target: CommentTarget) : ComposerMode

    data class Edit(val draft: ReviewDraft) : ComposerMode
}

enum class EditorTab { Write, Preview }

/** Everything the line comment sheet shows. */
data class ComposerState(
    val mode: ComposerMode,
    val text: String = "",
    val tab: EditorTab = EditorTab.Write,
    /** The text rendered by the core, refreshed when Preview is opened. */
    val preview: List<MdBlock> = emptyList(),
    /** Why the core could not render the Preview, if it could not. */
    val previewError: String? = null,
    /** Pending drafts other than this one. */
    val otherPending: Int,
    /** The pending review's head moved; new drafts are refused. */
    val stale: Boolean,
    val action: ActionState = ActionState.Idle,
) {
    val chip: String
        get() = when (mode) {
            is ComposerMode.New -> ReviewLabels.anchorChip(mode.target)
            is ComposerMode.Edit -> ReviewLabels.anchorChip(mode.draft.anchor)
        }

    val note: String
        get() = when (mode) {
            is ComposerMode.New -> ReviewLabels.anchorNote(mode.target.startLine, mode.target.line, mode.target.side)
            is ComposerMode.Edit -> mode.draft.anchor.let { ReviewLabels.anchorNote(it.startLine, it.line, it.side) }
        }

    /** The count on "Add to review": the review's size once this is added. */
    val badge: Int get() = otherPending + 1

    val blockedReason: String?
        get() = if (mode is ComposerMode.New && stale) {
            "New commits arrived after your pending comments were written. Discard them from Finish review before adding more."
        } else {
            null
        }

    /**
     * "Comment now" goes out as a review, which would take the other pending
     * drafts with it, so it is offered only when there are none.
     */
    val commentNowNote: String?
        get() = if (mode is ComposerMode.New && otherPending > 0) {
            "Comment now is off while your ${ReviewLabels.pendingCount(otherPending)} wait: it would send them too."
        } else {
            null
        }

    val canSend: Boolean get() = text.isNotBlank() && !action.running && blockedReason == null

    val canCommentNow: Boolean get() = canSend && mode is ComposerMode.New && otherPending == 0
}

/**
 * The line comment sheet's logic, shared by the diff (new comments, edits)
 * and the submit sheet (edits). Not a ViewModel: it lives in its owner's
 * [scope] and reports back through the callbacks.
 *
 * @param onPendingChanged the pending review after any change.
 * @param onPosted a review went out (Comment now); threads changed.
 * @param onClosed the sheet closed, saved or not.
 */
class CommentComposer(
    private val backend: RostrumBackend,
    private val pr: PrRef,
    private val scope: CoroutineScope,
    private val messages: Messages,
    private val onPendingChanged: (PendingReview) -> Unit,
    private val onPosted: () -> Unit,
    private val onClosed: () -> Unit,
) {
    private val _state = MutableStateFlow<ComposerState?>(null)

    /** `null` while the sheet is closed. */
    val state: StateFlow<ComposerState?> = _state.asStateFlow()

    fun openNew(target: CommentTarget, pending: PendingReview) {
        _state.value = ComposerState(ComposerMode.New(target), otherPending = pending.drafts.size, stale = pending.stale)
    }

    fun openEdit(draft: ReviewDraft, pending: PendingReview) {
        _state.value = ComposerState(
            mode = ComposerMode.Edit(draft),
            text = draft.body,
            otherPending = (pending.drafts.size - 1).coerceAtLeast(0),
            stale = pending.stale,
        )
    }

    fun setText(text: String) = _state.update { it?.copy(text = text, action = ActionState.Idle) }

    fun setTab(tab: EditorTab) = _state.update { current ->
        when {
            current == null -> null
            tab != EditorTab.Preview -> current.copy(tab = tab)
            else -> when (val rendered = backend.renderMarkdown(current.text, pr.repo)) {
                is Outcome.Ok -> current.copy(tab = tab, preview = rendered.value, previewError = null)
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "preview_failed", "pr" to pr, "error" to rendered.error::class.simpleName)
                    current.copy(tab = tab, preview = emptyList(), previewError = rendered.error.describe())
                }
            }
        }
    }

    fun dismiss() = close()

    /** Keep the comment as a pending draft. */
    fun addToReview() {
        val current = _state.value ?: return
        val mode = current.mode as? ComposerMode.New ?: return
        if (!current.canSend) return
        running()
        scope.launch {
            when (val added = backend.addDraft(pr, mode.target.anchor, mode.target.rangeStart, current.text.trim())
                .logErr(TAG, "draft_add_failed", "pr" to pr)) {
                is Outcome.Err -> failed(added.error)
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "draft_added", "pr" to pr, "path" to mode.target.path, "line" to mode.target.line)
                    onPendingChanged(added.value)
                    close()
                }
            }
        }
    }

    /**
     * Post the comment at once. The core only posts inline comments as part
     * of a review, so this adds the draft and submits a Comment review; it is
     * refused while other drafts are pending, which would go out with it. If
     * the submit fails the draft stays pending.
     */
    fun commentNow() {
        val current = _state.value ?: return
        val mode = current.mode as? ComposerMode.New ?: return
        if (!current.canCommentNow) return
        running()
        scope.launch {
            val added = when (val outcome = backend.addDraft(pr, mode.target.anchor, mode.target.rangeStart, current.text.trim())) {
                is Outcome.Err -> {
                    outcome.logErr(TAG, "comment_now_failed", "pr" to pr)
                    return@launch failed(outcome.error)
                }
                is Outcome.Ok -> outcome.value
            }
            when (val sent = backend.submitReview(pr, ReviewEvent.Comment, "", includeDrafts = true)) {
                is Outcome.Err -> {
                    sent.logErr(TAG, "comment_now_submit_failed", "pr" to pr)
                    onPendingChanged(added)
                    messages.send("Added to your review, but sending failed: ${sent.error.describe()}")
                    close()
                }
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "comment_posted", "pr" to pr, "drafts" to added.drafts.size)
                    (backend.pendingReview(pr) as? Outcome.Ok)?.let { onPendingChanged(it.value) }
                    messages.send("Comment posted")
                    onPosted()
                    close()
                }
            }
        }
    }

    /** Save an edited draft. */
    fun save() {
        val current = _state.value ?: return
        val mode = current.mode as? ComposerMode.Edit ?: return
        if (!current.canSend) return
        running()
        scope.launch {
            when (val edited = backend.editDraft(pr, mode.draft.id, current.text.trim()).logErr(TAG, "draft_edit_failed", "pr" to pr)) {
                is Outcome.Err -> failed(edited.error)
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "draft_edited", "pr" to pr, "draft" to mode.draft.id)
                    onPendingChanged(edited.value)
                    close()
                }
            }
        }
    }

    /** Delete the draft being edited. */
    fun delete() {
        val current = _state.value ?: return
        val mode = current.mode as? ComposerMode.Edit ?: return
        running()
        scope.launch {
            when (val removed = backend.removeDraft(pr, mode.draft.id).logErr(TAG, "draft_remove_failed", "pr" to pr)) {
                is Outcome.Err -> failed(removed.error)
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "draft_removed", "pr" to pr, "draft" to mode.draft.id)
                    onPendingChanged(removed.value)
                    messages.send("Pending comment deleted")
                    close()
                }
            }
        }
    }

    private fun running() = _state.update { it?.copy(action = ActionState.Running) }

    private fun failed(error: BackendError) = _state.update { it?.copy(action = ActionState.Failed(error)) }

    private fun close() {
        if (_state.value == null) return
        _state.value = null
        onClosed()
    }

    private companion object {
        const val TAG = "RostrumReview"
    }
}
