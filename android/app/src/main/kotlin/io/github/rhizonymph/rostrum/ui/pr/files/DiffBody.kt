package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import java.time.Instant

/** What the diff body reports back; every callback takes a row index into the rows. */
class DiffBodyActions(
    val onLineTap: (Int) -> Unit,
    val onSelectionStart: (Int) -> Unit,
    val onSelectionMove: (Int) -> Unit,
    val onSelectionEnd: () -> Unit,
    val onSelectionCancel: () -> Unit,
    val onEditDraft: (Long) -> Unit,
    val onDeleteDraft: (Long) -> Unit,
    val onStartReply: (String) -> Unit,
    val onReplyText: (String) -> Unit,
    val onSendReply: () -> Unit,
    val onCancelReply: () -> Unit,
)

/**
 * The file's rows in one lazy list. Gestures are handled here rather than per
 * row: a tap in the gutter comments on that line; a long press in the gutter
 * followed by a vertical drag selects a range (the list does not scroll
 * while selecting). Unwrapped, every code line shares one horizontal offset,
 * dragged sideways anywhere on the list.
 */
@Composable
fun DiffBody(
    rows: List<DiffRow>,
    softWrap: Boolean,
    selectedRows: Set<Int>,
    reply: ThreadReply?,
    now: Instant,
    actions: DiffBodyActions,
    modifier: Modifier = Modifier,
    listState: LazyListState = rememberLazyListState(),
) {
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer()
    val longest = remember(rows) {
        rows.asSequence().filterIsInstance<DiffRow.Line>().map { expandTabs(it.line.text) }.maxByOrNull { it.length }.orEmpty()
    }
    val codeWidthPx = remember(longest, measurer) {
        if (longest.isEmpty()) 0 else measurer.measure(longest, RostrumText.diffLine, softWrap = false, maxLines = 1).size.width
    }
    var scrollX by remember { mutableFloatStateOf(0f) }
    val current by rememberUpdatedState(actions)
    val currentRows by rememberUpdatedState(rows)

    BoxWithConstraints(modifier.fillMaxSize()) {
        val gutterPx = with(density) { (GUTTER_WIDTH + MARKER_WIDTH).dp.toPx() }
        val endPaddingPx = with(density) { 12.dp.toPx() }
        val maxScroll = (codeWidthPx + endPaddingPx - (constraints.maxWidth - gutterPx)).coerceAtLeast(0f)
        // Read clamped, so a narrower window (rotation) never shows past the end.
        val offset = { scrollX.coerceIn(0f, maxScroll) }
        val horizontal = rememberScrollableState { delta ->
            val before = offset()
            scrollX = (before - delta).coerceIn(0f, maxScroll)
            before - scrollX
        }

        fun rowAt(offset: Offset): Int? = listState.layoutInfo.visibleItemsInfo
            .firstOrNull { offset.y >= it.offset && offset.y < it.offset + it.size }
            ?.index

        LazyColumn(
            state = listState,
            modifier = Modifier
                .fillMaxSize()
                .scrollable(horizontal, Orientation.Horizontal, enabled = !softWrap && maxScroll > 0f)
                .pointerInput(gutterPx) {
                    detectTapGestures { offset ->
                        if (offset.x <= gutterPx) rowAt(offset)?.let { current.onLineTap(it) }
                    }
                }
                .pointerInput(gutterPx) {
                    var selecting = false
                    detectDragGesturesAfterLongPress(
                        onDragStart = { offset ->
                            val row = rowAt(offset)
                            selecting = offset.x <= gutterPx && row != null && currentRows.getOrNull(row) is DiffRow.Line
                            if (selecting && row != null) current.onSelectionStart(row)
                        },
                        onDrag = { change, _ ->
                            if (selecting) {
                                change.consume()
                                rowAt(change.position)?.let { current.onSelectionMove(it) }
                            }
                        },
                        onDragEnd = {
                            if (selecting) current.onSelectionEnd()
                            selecting = false
                        },
                        onDragCancel = {
                            if (selecting) current.onSelectionCancel()
                            selecting = false
                        },
                    )
                },
        ) {
            itemsIndexed(rows, key = { index, row -> rowKey(index, row) }, contentType = { _, row -> row::class }) { index, row ->
                when (row) {
                    is DiffRow.Hunk -> HunkHeaderRow(row.header, softWrap)
                    is DiffRow.Line -> DiffLineRow(
                        line = row.line,
                        selected = index in selectedRows,
                        softWrap = softWrap,
                        scrollX = offset,
                        onComment = if (row.line.anchor != null) ({ current.onLineTap(index) }) else null,
                    )
                    is DiffRow.Thread -> ThreadCard(
                        thread = row.thread,
                        reply = reply?.takeIf { it.threadId == row.thread.id },
                        now = now,
                        onStartReply = { current.onStartReply(row.thread.id) },
                        onReplyText = { current.onReplyText(it) },
                        onSendReply = { current.onSendReply() },
                        onCancelReply = { current.onCancelReply() },
                    )
                    is DiffRow.Draft -> DraftCard(
                        draft = row.draft,
                        onEdit = { current.onEditDraft(row.draft.id) },
                        onDelete = { current.onDeleteDraft(row.draft.id) },
                    )
                }
            }
        }
    }
}

/** Keys stable across reloads, so adding a draft does not reset the list. */
private fun rowKey(index: Int, row: DiffRow): Any = when (row) {
    is DiffRow.Hunk -> "hunk-${row.index}"
    is DiffRow.Line -> "line-${row.line.oldLine}-${row.line.newLine}-${row.line.kind}"
    is DiffRow.Thread -> "thread-${row.thread.id}"
    is DiffRow.Draft -> "draft-${row.draft.id}"
}.let { key -> if (row is DiffRow.Line && row.line.oldLine == null && row.line.newLine == null) "$key-$index" else key }
