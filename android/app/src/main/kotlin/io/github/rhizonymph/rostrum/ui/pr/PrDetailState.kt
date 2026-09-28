package io.github.rhizonymph.rostrum.ui.pr

import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeFormState
import java.time.Instant

/** Everything the pull request screen shows outside the Files tab and the local card. */
data class PrDetailUiState(
    val pr: PrRef,
    val detail: UiState<PullDetail> = UiState.Loading,
    /** A reload is in flight while [detail] still shows the previous data. */
    val refreshing: Boolean = false,
    /** When [detail] was last fetched; the Checks footnote says how long ago. */
    val loadedAt: Instant? = null,
    /** The composer's text. */
    val comment: String = "",
    val postingComment: Boolean = false,
    /** The inline reply being written, if any. */
    val reply: ReplyDraft? = null,
    /** The label picker, while open. */
    val labels: LabelPickerState? = null,
    /** A pull-request-level action in flight. */
    val busy: PrBusy? = null,
    /** The merge sheet, while open. */
    val merge: MergeFormState? = null,
)

data class ReplyDraft(val threadId: String, val text: String = "", val sending: Boolean = false)

data class LabelPickerState(
    val available: UiState<List<LabelView>>,
    /** The label being added or removed right now. */
    val pending: String? = null,
)

/** The pull-request-level actions that disable their controls while running. */
enum class PrBusy { Draft, Close, Reopen, UpdateMerge, UpdateRebase }
