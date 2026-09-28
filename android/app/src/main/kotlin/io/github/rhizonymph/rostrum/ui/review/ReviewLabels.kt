package io.github.rhizonymph.rostrum.ui.review

import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DraftAnchor
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.ui.format.fileName
import io.github.rhizonymph.rostrum.ui.format.lineRangeLabel
import io.github.rhizonymph.rostrum.ui.format.sideLabel

/**
 * What a new line comment attaches to: one line, or a range down one side of
 * one file ending at [anchor]. Both anchors come from the diff's rows, never
 * built by hand, so they are always ones the core can place.
 */
data class CommentTarget(val anchor: CommentAnchor, val rangeStart: CommentAnchor? = null) {
    init {
        if (rangeStart != null) {
            require(rangeStart.path == anchor.path && rangeStart.side == anchor.side) {
                "a range stays on one side of one file"
            }
            require(rangeStart.line < anchor.line) { "a range runs downwards" }
        }
    }

    val path: String get() = anchor.path
    val line: Int get() = anchor.line
    val side: Side get() = anchor.side
    val startLine: Int? get() = rangeStart?.line
}

/** Wording for line comments and pending drafts, shared by the diff and the sheets. */
object ReviewLabels {
    /** `overview.rs · L53–56 · new side`. */
    fun anchorChip(path: String, startLine: Int?, line: Int, side: Side): String =
        "${fileName(path)} · ${lineRangeLabel(startLine, line)} · ${sideLabel(side)}"

    fun anchorChip(target: CommentTarget): String = anchorChip(target.path, target.startLine, target.line, target.side)

    fun anchorChip(anchor: DraftAnchor): String = anchorChip(anchor.path, anchor.startLine, anchor.line, anchor.side)

    /** `overview.rs:L53–56`. */
    fun location(anchor: DraftAnchor): String = "${fileName(anchor.path)}:${lineRangeLabel(anchor.startLine, anchor.line)}"

    /** `lines 53–56` or `line 60`. */
    fun linesPhrase(startLine: Int?, line: Int): String =
        if (startLine == null || startLine == line) "line $line" else "lines $startLine–$line"

    fun editDescription(anchor: DraftAnchor): String =
        "Edit comment on ${fileName(anchor.path)} ${linesPhrase(anchor.startLine, anchor.line)}"

    fun deleteDescription(anchor: DraftAnchor): String =
        "Delete comment on ${fileName(anchor.path)} ${linesPhrase(anchor.startLine, anchor.line)}"

    /** The note under the editor saying where the comment will attach. */
    fun anchorNote(startLine: Int?, line: Int, side: Side): String {
        val lines = linesPhrase(startLine, line)
        return when (side) {
            Side.Right -> "Anchored to new-file $lines (RIGHT). Removed lines anchor LEFT."
            Side.Left -> "Anchored to old-file $lines (LEFT). Added and unchanged lines anchor RIGHT."
        }
    }

    /** `1 pending comment`, `2 pending comments`. */
    fun pendingCount(count: Int): String = if (count == 1) "1 pending comment" else "$count pending comments"
}
