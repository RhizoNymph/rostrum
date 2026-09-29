package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.data.model.ThreadCommentView
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import io.github.rhizonymph.rostrum.ui.review.ReviewLabels
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

private val InlineCardShape = RoundedCornerShape(12.dp)

@Composable
private fun InlineCard(modifier: Modifier = Modifier, border: Color = RostrumTheme.colors.border, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = modifier
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .fillMaxWidth()
            .clip(InlineCardShape)
            .background(RostrumTheme.colors.surface)
            .border(1.dp, border, InlineCardShape)
            .padding(start = 12.dp, end = 12.dp, top = 12.dp, bottom = 4.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        content = content,
    )
}

/**
 * An existing review thread at its line: the comments, and Reply (the core
 * cannot resolve threads, so there is no Resolve button).
 */
@Composable
fun ThreadCard(
    thread: ReviewThreadView,
    reply: ThreadReply?,
    now: Instant,
    onStartReply: () -> Unit,
    onReplyText: (String) -> Unit,
    onSendReply: () -> Unit,
    onCancelReply: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val line = thread.line ?: thread.originalLine
    InlineCard(modifier.semantics(mergeDescendants = false) { contentDescription = "Review thread on line ${line ?: "?"}" }) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (line != null) Text("L$line", style = RostrumText.mono12, color = colors.textMuted)
            when {
                thread.outdated -> StatusChip("Outdated", ColorRole.Neutral)
                thread.resolved -> StatusChip("Resolved", ColorRole.Success)
                else -> StatusChip("Unresolved", ColorRole.Warning)
            }
        }
        thread.comments.forEach { ThreadComment(it, now) }
        if (reply != null) {
            ReplyEditor(reply, onReplyText, onSendReply, onCancelReply)
        } else if (thread.canReply) {
            Row(Modifier.padding(start = 22.dp)) {
                TextPillButton("Reply", onStartReply, style = RostrumText.button.copy(fontSize = 13.sp))
            }
        } else {
            Spacer(Modifier.padding(bottom = 4.dp))
        }
    }
}

@Composable
private fun ThreadComment(comment: ThreadCommentView, now: Instant) {
    val colors = RostrumTheme.colors
    val login = comment.author?.login ?: "ghost"
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Avatar(login, size = 24.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                buildAnnotatedString {
                    withStyle(RostrumText.meta.toSpanStyle().copy(fontWeight = FontWeight.SemiBold, color = colors.text)) { append(login) }
                    withStyle(RostrumText.meta.toSpanStyle().copy(color = colors.textMuted)) { append(" · ${relativeAge(comment.createdAt, now)}") }
                },
                style = RostrumText.meta.copy(lineHeight = 17.sp),
            )
            MarkdownBlocks(comment.body, textStyle = RostrumText.meta)
        }
    }
}

@Composable
private fun ReplyEditor(reply: ThreadReply, onText: (String) -> Unit, onSend: () -> Unit, onCancel: () -> Unit) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(bottom = 8.dp)) {
        RostrumTextField(
            value = reply.text,
            onValueChange = onText,
            placeholder = "Reply…",
            accessibilityLabel = "Reply to this thread",
            singleLine = false,
            minHeight = 72.dp,
            fill = colors.bg,
            modifier = Modifier.fillMaxWidth(),
        )
        (reply.action as? ActionState.Failed)?.let { FieldError(it.error.describe()) }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Spacer(Modifier.weight(1f))
            TextPillButton("Cancel", onCancel)
            Spacer(Modifier.width(4.dp))
            PrimaryButton("Reply", onSend, enabled = reply.text.isNotBlank(), busy = reply.action.running)
        }
    }
}

/** One of your pending drafts, shown under its (last) line. */
@Composable
fun DraftCard(draft: ReviewDraft, onEdit: () -> Unit, onDelete: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    InlineCard(modifier, border = colors.accent.copy(alpha = 0.35f)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            StatusChip("Pending", ColorRole.Accent)
            Text(ReviewLabels.location(draft.anchor), style = RostrumText.mono12, color = colors.textMuted, maxLines = 1)
            Spacer(Modifier.weight(1f))
            RostrumIconButton(RostrumIcons.Edit, ReviewLabels.editDescription(draft.anchor), onEdit, tint = colors.textMuted, iconSize = 18.dp)
            RostrumIconButton(RostrumIcons.Close, ReviewLabels.deleteDescription(draft.anchor), onDelete, tint = colors.textMuted, iconSize = 18.dp)
        }
        Text(draft.body, style = RostrumText.meta, color = colors.textSecondary, modifier = Modifier.padding(bottom = 10.dp))
    }
}
