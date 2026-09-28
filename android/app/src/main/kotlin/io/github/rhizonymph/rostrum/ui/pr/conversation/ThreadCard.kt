package io.github.rhizonymph.rostrum.ui.pr.conversation

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import io.github.rhizonymph.rostrum.ui.pr.ReplyDraft
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** Callbacks for replying inside a thread card. */
data class ReplyActions(
    val onStart: (threadId: String) -> Unit,
    val onText: (String) -> Unit,
    val onSend: () -> Unit,
    val onCancel: () -> Unit,
)

/**
 * An inline review thread outside the diff: its location and state, the code
 * it is anchored to, its comments, and a reply box.
 */
@Composable
fun ThreadCard(
    thread: ReviewThreadView,
    now: Instant,
    reply: ReplyDraft?,
    actions: ReplyActions,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                thread.path,
                style = RostrumText.mono12,
                color = colors.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.StartEllipsis,
                modifier = Modifier.weight(1f),
            )
            (thread.line ?: thread.originalLine)?.let {
                Text("L$it", style = RostrumText.mono12, color = colors.textMuted)
            }
            when {
                thread.resolved -> StatusChip("Resolved", ColorRole.Success)
                thread.outdated -> StatusChip("Outdated", ColorRole.Neutral)
                else -> StatusChip("Unresolved", ColorRole.Warning)
            }
        }
        CardDivider()
        Column(
            Modifier.padding(start = 14.dp, end = 14.dp, top = 12.dp, bottom = 4.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            thread.comments.forEach { comment ->
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        val login = comment.author?.login ?: "ghost"
                        Avatar(login, size = 24.dp)
                        Text(login, style = RostrumText.meta.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
                        Text("· ${relativeAge(comment.createdAt, now)}", style = RostrumText.meta, color = colors.textMuted)
                    }
                    MarkdownBlocks(
                        comment.body,
                        modifier = Modifier.padding(start = 32.dp),
                        textStyle = RostrumText.body.copy(lineHeight = 20.sp),
                    )
                }
            }
            if (reply != null && reply.threadId == thread.id) {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(bottom = 10.dp)) {
                    RostrumTextField(
                        value = reply.text,
                        onValueChange = actions.onText,
                        placeholder = "Reply…",
                        singleLine = false,
                        minHeight = 72.dp,
                        fill = colors.bg,
                        enabled = !reply.sending,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        TextPillButton("Cancel", actions.onCancel, enabled = !reply.sending)
                        PrimaryButton("Reply", actions.onSend, enabled = reply.text.isNotBlank(), busy = reply.sending)
                    }
                }
            } else if (thread.canReply) {
                TextPillButton("Reply…", onClick = { actions.onStart(thread.id) }, modifier = Modifier.padding(start = 16.dp))
            }
        }
    }
}
