package io.github.rhizonymph.rostrum.ui.pr.conversation

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.ReviewState
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.data.model.TimelineEntry
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.ui.components.CommentCard
import io.github.rhizonymph.rostrum.ui.components.EventRow
import io.github.rhizonymph.rostrum.ui.components.eventIcon
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import io.github.rhizonymph.rostrum.ui.pr.ReplyDraft
import java.time.Instant

/**
 * The Conversation tab: the summary block, then the timeline oldest first.
 * Reviews show the threads they opened; threads no review claims follow at
 * the end, so none is ever hidden.
 */
@Composable
fun ConversationTab(
    detail: PullDetail,
    now: Instant,
    reply: ReplyDraft?,
    replyActions: ReplyActions,
    onAddLabel: () -> Unit,
    onMerge: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val threadsById = detail.threads.associateBy { it.id }
    val claimed = detail.timeline.flatMap { (it.kind as? TimelineKind.Review)?.threadIds.orEmpty() }.toSet()
    val orphans = detail.threads.filter { it.id !in claimed }
    LazyColumn(
        modifier = modifier,
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "summary") {
            PrSummaryBlock(detail.header, now, onAddLabel, onMerge)
        }
        detail.timeline.forEach { entry -> timelineEntry(entry, threadsById, now, reply, replyActions) }
        orphans.forEach { thread ->
            item(key = "orphan-${thread.id}") { ThreadCard(thread, now, reply, replyActions) }
        }
    }
}

private fun LazyListScope.timelineEntry(
    entry: TimelineEntry,
    threadsById: Map<String, ReviewThreadView>,
    now: Instant,
    reply: ReplyDraft?,
    replyActions: ReplyActions,
) {
    val actor = entry.author?.login ?: "ghost"
    val age = relativeAge(entry.createdAt, now)
    when (val kind = entry.kind) {
        is TimelineKind.Description -> item(key = entry.id) { CommentCard(actor, entry.createdAt, now, kind.body) }
        is TimelineKind.Comment -> item(key = entry.id) { CommentCard(actor, entry.createdAt, now, kind.body) }
        is TimelineKind.Event -> item(key = entry.id) {
            EventRow(
                icon = eventIcon(kind.event),
                actor = actor,
                text = kind.text,
                age = age,
            )
        }
        is TimelineKind.Review -> {
            item(key = entry.id) {
                EventRow(
                    icon = RostrumIcons.Eye,
                    actor = actor,
                    text = reviewVerb(kind.state),
                    age = age,
                    chip = kind.chip.takeIf { it.role != ColorRole.Neutral },
                )
            }
            if (kind.body.isNotEmpty()) {
                item(key = "${entry.id}-body") { CommentCard(actor, entry.createdAt, now, kind.body) }
            }
            kind.threadIds.mapNotNull(threadsById::get).forEach { thread ->
                item(key = "${entry.id}-${thread.id}") { ThreadCard(thread, now, reply, replyActions) }
            }
        }
    }
}

private fun reviewVerb(state: ReviewState): String = when (state) {
    ReviewState.Approved -> "approved these changes"
    ReviewState.ChangesRequested -> "requested changes"
    ReviewState.Commented -> "reviewed"
    ReviewState.Dismissed -> "had a review dismissed"
    ReviewState.Pending -> "started a review"
}
