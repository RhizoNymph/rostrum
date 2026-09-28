package io.github.rhizonymph.rostrum.ui.review

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the sheet is about: the pull request and your pending drafts on it. */
data class SubmitContent(val header: PullHeader, val pending: PendingReview)

data class SubmitReviewState(
    val content: UiState<SubmitContent> = UiState.Loading,
    val summary: String = "",
    val verdict: ReviewEvent = ReviewEvent.Comment,
    val submit: ActionState = ActionState.Idle,
    val discard: ActionState = ActionState.Idle,
    val confirmingDiscard: Boolean = false,
) {
    private val loaded: SubmitContent? get() = (content as? UiState.Loaded)?.data

    /** Why [event] cannot be chosen now, or `null`. */
    fun blockedReason(event: ReviewEvent): String? = loaded?.let { ReviewRules.blockedReason(event, it.header, it.pending) }

    val staleWarning: String? get() = loaded?.let { ReviewRules.staleWarning(it.header, it.pending) }

    /** Whether the pending drafts go out with this review. */
    val includesDrafts: Boolean get() = loaded?.pending?.let(ReviewRules::includesDrafts) ?: false

    val canSubmit: Boolean
        get() {
            val content = loaded ?: return false
            return !submit.running && !discard.running && blockedReason(verdict) == null &&
                ReviewRules.hasContent(verdict, summary, content.pending)
        }
}

/**
 * The "Finish your review" sheet: pending drafts (editable through
 * [composer]), a summary, the verdict, and submit or discard.
 */
class SubmitReviewViewModel(
    private val backend: RostrumBackend,
    private val pr: PrRef,
) : ViewModel() {
    private val _state = MutableStateFlow(SubmitReviewState())
    val state: StateFlow<SubmitReviewState> = _state.asStateFlow()

    val messages = Messages()

    private val _submitted = Channel<Unit>(Channel.BUFFERED)

    /** Fires once per accepted review. */
    val submitted: Flow<Unit> = _submitted.receiveAsFlow()

    val composer = CommentComposer(
        backend = backend,
        pr = pr,
        scope = viewModelScope,
        messages = messages,
        onPendingChanged = ::applyPending,
        onPosted = ::load,
        onClosed = {},
    )

    /** Load (or refresh, keeping what is shown) the header and the pending review. */
    fun load() {
        if (_state.value.content !is UiState.Loaded) _state.update { it.copy(content = UiState.Loading) }
        viewModelScope.launch {
            val header = backend.pullHeader(pr).logErr(TAG, "review_header_failed", "pr" to pr)
            val pending = backend.pendingReview(pr).logErr(TAG, "review_pending_failed", "pr" to pr)
            val content: UiState<SubmitContent> = when {
                header is Outcome.Err -> UiState.Error(header.error)
                pending is Outcome.Err -> UiState.Error(pending.error)
                header is Outcome.Ok && pending is Outcome.Ok -> UiState.Loaded(SubmitContent(header.value, pending.value))
                else -> UiState.Loading
            }
            _state.update { it.copy(content = content).withAllowedVerdict() }
        }
    }

    fun setSummary(summary: String) = _state.update { it.copy(summary = summary, submit = ActionState.Idle) }

    /** Choose a verdict; a blocked one is ignored. */
    fun setVerdict(event: ReviewEvent) = _state.update {
        if (it.blockedReason(event) != null) it else it.copy(verdict = event, submit = ActionState.Idle)
    }

    fun submit() {
        val current = _state.value
        if (!current.canSubmit) return
        val include = current.includesDrafts
        _state.update { it.copy(submit = ActionState.Running) }
        viewModelScope.launch {
            when (val sent = backend.submitReview(pr, current.verdict, current.summary.trim(), include)
                .logErr(TAG, "review_submit_failed", "pr" to pr, "verdict" to current.verdict)) {
                is Outcome.Err -> _state.update { it.copy(submit = ActionState.Failed(sent.error)) }
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "review_submitted", "pr" to pr, "verdict" to current.verdict, "drafts" to include)
                    _state.update { it.copy(submit = ActionState.Idle, summary = "", verdict = ReviewEvent.Comment) }
                    messages.send(
                        when (current.verdict) {
                            ReviewEvent.Comment -> "Review submitted"
                            ReviewEvent.Approve -> "Approved #${pr.number}"
                            ReviewEvent.RequestChanges -> "Requested changes on #${pr.number}"
                        },
                    )
                    load()
                    _submitted.trySend(Unit)
                }
            }
        }
    }

    fun requestDiscard() = _state.update { it.copy(confirmingDiscard = true) }

    fun cancelDiscard() = _state.update { it.copy(confirmingDiscard = false) }

    fun confirmDiscard() {
        _state.update { it.copy(confirmingDiscard = false, discard = ActionState.Running) }
        viewModelScope.launch {
            when (val discarded = backend.discardDrafts(pr).logErr(TAG, "drafts_discard_failed", "pr" to pr)) {
                is Outcome.Err -> _state.update { it.copy(discard = ActionState.Failed(discarded.error)) }
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "drafts_discarded", "pr" to pr)
                    _state.update { it.copy(discard = ActionState.Idle) }
                    applyPending(discarded.value)
                    messages.send("Pending comments discarded")
                }
            }
        }
    }

    /** Open a pending draft in the line comment editor. */
    fun editDraft(draftId: Long) {
        val pending = ((_state.value.content as? UiState.Loaded)?.data ?: return).pending
        val draft = pending.drafts.firstOrNull { it.id == draftId } ?: return
        composer.openEdit(draft, pending)
    }

    private fun applyPending(pending: PendingReview) = _state.update { state ->
        val loaded = (state.content as? UiState.Loaded)?.data ?: return@update state
        state.copy(content = UiState.Loaded(loaded.copy(pending = pending))).withAllowedVerdict()
    }

    /** A verdict that became blocked (drafts went stale) falls back to Comment. */
    private fun SubmitReviewState.withAllowedVerdict(): SubmitReviewState =
        if (blockedReason(verdict) != null) copy(verdict = ReviewEvent.Comment) else this

    private companion object {
        const val TAG = "RostrumReview"
    }
}
