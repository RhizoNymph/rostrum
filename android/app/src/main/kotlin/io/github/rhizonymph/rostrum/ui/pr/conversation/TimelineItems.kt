package io.github.rhizonymph.rostrum.ui.pr.conversation

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.CardShape
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** The description or a comment: raised card, small avatar, author, age, markdown. */
@Composable
fun CommentCard(
    author: String,
    createdAt: Instant,
    now: Instant,
    body: List<MdBlock>,
    modifier: Modifier = Modifier,
    chip: Chip? = null,
) {
    val colors = RostrumTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(colors.raised, CardShape)
            .border(1.dp, colors.border, CardShape)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Avatar(author, size = 20.dp)
            Text(author, style = RostrumText.meta.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
            Text("· ${relativeAge(createdAt, now)}", style = RostrumText.meta, color = colors.textMuted)
            if (chip != null) StatusChip(chip)
        }
        if (body.isEmpty()) {
            Text("No description provided.", style = RostrumText.body, color = colors.textSubtle)
        } else {
            MarkdownBlocks(body)
        }
    }
}

/**
 * A one-line event: a bordered circle with an icon, "**actor** text · age",
 * and optional trailing content (commit SHAs, a review chip).
 */
@Composable
fun EventRow(
    icon: ImageVector,
    actor: String,
    text: String,
    age: String,
    modifier: Modifier = Modifier,
    chip: Chip? = null,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier.fillMaxWidth().padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier.size(28.dp).background(colors.surface, CircleShape).border(1.dp, colors.border, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = null, tint = colors.textMuted, modifier = Modifier.size(14.dp))
        }
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = colors.textSecondary, fontWeight = FontWeight.Medium)) { append(actor) }
                append(" $text · $age")
            },
            style = RostrumText.meta,
            color = colors.textMuted,
            modifier = Modifier.weight(1f),
        )
        if (chip != null) StatusChip(chip)
    }
}

/** The icon for an event row. */
fun eventIcon(event: TimelineEvent): ImageVector = when (event) {
    TimelineEvent.Merged -> RostrumIcons.Merge
    TimelineEvent.Closed -> RostrumIcons.Close
    TimelineEvent.Reopened -> RostrumIcons.Refresh
    TimelineEvent.ReadyForReview, is TimelineEvent.ReviewRequested -> RostrumIcons.Eye
    TimelineEvent.ConvertedToDraft, is TimelineEvent.Renamed -> RostrumIcons.Edit
    TimelineEvent.ForcePushed -> RostrumIcons.Rebase
    is TimelineEvent.Other -> RostrumIcons.Commit
    is TimelineEvent.Assigned, is TimelineEvent.Labeled -> RostrumIcons.Plus
    is TimelineEvent.Unlabeled -> RostrumIcons.Close
}
