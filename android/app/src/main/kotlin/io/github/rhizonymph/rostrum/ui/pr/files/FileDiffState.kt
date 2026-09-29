package io.github.rhizonymph.rostrum.ui.pr.files

import io.github.rhizonymph.rostrum.data.model.ChangedFile
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.FileDiff
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull

/** Where the open file sits in the overview's ranked order. */
data class FileNav(val order: List<Int>, val position: Int) {
    init {
        require(position in order.indices) { "position $position outside ${order.size} files" }
    }

    /** `1/7`. */
    val label: String get() = "${position + 1}/${order.size}"
    val hasPrevious: Boolean get() = position > 0
    val hasNext: Boolean get() = position < order.lastIndex
    val previous: Int? get() = order.getOrNull(position - 1)
    val next: Int? get() = order.getOrNull(position + 1)

    companion object {
        /** The nav for [fileIndex], or `null` when it is not in [order]. */
        fun of(order: List<Int>, fileIndex: Int): FileNav? =
            order.indexOf(fileIndex).takeIf { it >= 0 }?.let { FileNav(order, it) }
    }
}

/** An inline reply being written under a thread. */
data class ThreadReply(
    val threadId: String,
    val text: String = "",
    val action: ActionState = ActionState.Idle,
)

/** The single-file diff screen. */
data class FileDiffState(
    val fileIndex: Int,
    /** `null` until the overview has answered (or when it failed). */
    val nav: FileNav? = null,
    /** The open file's stats, from the overview or the diff, for the header. */
    val file: ChangedFile? = null,
    val diff: UiState<FileDiff> = UiState.Loading,
    val softWrap: Boolean = false,
    /** Files marked viewed in this session (not synced to GitHub). */
    val viewed: Set<Int> = emptySet(),
    val selection: LineSelection? = null,
    /** Rows highlighted by [selection]. */
    val selectedRows: Set<Int> = emptySet(),
    val pending: PendingReview? = null,
    val reply: ThreadReply? = null,
) {
    val isViewed: Boolean get() = fileIndex in viewed

    val rows: List<DiffRow> get() = (diff.dataOrNull()?.body as? FileDiffBody.Rows)?.rows.orEmpty()
}
