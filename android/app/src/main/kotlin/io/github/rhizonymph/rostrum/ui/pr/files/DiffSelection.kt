package io.github.rhizonymph.rostrum.ui.pr.files

import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.ui.review.CommentTarget

/** A line of the diff that can take a comment: its row, its hunk, and the anchor the core gave it. */
data class LineSlot(val row: Int, val hunk: Int, val anchor: CommentAnchor)

/**
 * Lines picked for one comment by long-pressing a line number and dragging.
 * It stays on the [side] and within the [hunk] where it started; rows of the
 * other side in between are skipped rather than ending the range.
 */
data class LineSelection(val side: Side, val hunk: Int, val originRow: Int, val currentRow: Int) {
    val firstRow: Int get() = minOf(originRow, currentRow)
    val lastRow: Int get() = maxOf(originRow, currentRow)
}

/** Range selection over a file's rows. Pure: rows in, rows and anchors out. */
object DiffSelection {
    /** Every commentable line, keyed by row index. */
    fun slots(rows: List<DiffRow>): Map<Int, LineSlot> {
        val out = LinkedHashMap<Int, LineSlot>()
        var hunk = -1
        rows.forEachIndexed { index, row ->
            when (row) {
                is DiffRow.Hunk -> hunk = row.index
                is DiffRow.Line -> row.line.anchor?.let { out[index] = LineSlot(index, hunk, it) }
                is DiffRow.Thread, is DiffRow.Draft -> Unit
            }
        }
        return out
    }

    /** Start a selection at [row], or `null` when that line cannot take a comment. */
    fun start(rows: List<DiffRow>, row: Int): LineSelection? {
        val slot = slots(rows)[row] ?: return null
        return LineSelection(slot.anchor.side, slot.hunk, row, row)
    }

    /** Move the selection's moving end to [row], clamped to the selection's hunk. */
    fun extend(selection: LineSelection, rows: List<DiffRow>, row: Int): LineSelection {
        val inHunk = slots(rows).values.filter { it.hunk == selection.hunk }.map { it.row }
        if (inHunk.isEmpty()) return selection
        return selection.copy(currentRow = row.coerceIn(inHunk.min(), inHunk.max()))
    }

    /** The rows the selection covers: its side's lines between its two ends. */
    fun selectedRows(selection: LineSelection, rows: List<DiffRow>): Set<Int> =
        slots(rows).values
            .filter { it.hunk == selection.hunk && it.anchor.side == selection.side && it.row in selection.firstRow..selection.lastRow }
            .mapTo(LinkedHashSet()) { it.row }

    /** What a comment on the selection attaches to: its last line, starting at its first. */
    fun target(selection: LineSelection, rows: List<DiffRow>): CommentTarget? {
        val slots = slots(rows)
        val anchors = selectedRows(selection, rows).mapNotNull { slots[it]?.anchor }.sortedBy { it.line }
        val last = anchors.lastOrNull() ?: return null
        val first = anchors.first()
        return CommentTarget(last, first.takeIf { it.line < last.line })
    }
}
