package io.github.rhizonymph.rostrum.ui.items

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.ui.components.LabelChip
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/**
 * One issue: status chip and title, `#21 · ada-lin · 3h` with the comment
 * count on the right, then its labels and assignees.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun IssueRow(issue: IssueSummary, now: Instant, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(
        modifier = modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 14.dp, vertical = 13.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            StatusChip(issue.statusChip, Modifier.padding(top = 1.dp))
            Text(issue.title, style = RostrumText.rowTitle, color = colors.text, modifier = Modifier.weight(1f))
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("#${issue.number}", style = RostrumText.mono12, color = colors.textMuted)
            Dot(colors)
            Text(issueAuthorLabel(issue), style = RostrumText.meta, color = colors.textMuted, maxLines = 1)
            Dot(colors)
            Text(relativeAge(issue.createdAt, now), style = RostrumText.meta, color = colors.textMuted)
            issue.milestone?.let {
                Dot(colors)
                Text(it, style = RostrumText.meta, color = colors.textMuted, maxLines = 1)
            }
            Spacer(Modifier.weight(1f))
            commentsText(issue.commentCount)?.let { text ->
                Row(
                    modifier = Modifier.clearAndSetSemantics { contentDescription = text },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Icon(RostrumIcons.Comment, contentDescription = null, tint = colors.textMuted, modifier = Modifier.size(14.dp))
                    Text(issue.commentCount.toString(), style = RostrumText.mono12, color = colors.textMuted)
                }
            }
        }
        if (issue.labels.isNotEmpty() || issue.assignees.isNotEmpty()) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                issue.labels.forEach { LabelChip(it) }
                if (issue.assignees.isNotEmpty()) {
                    Text("→ ${assigneesText(issue)}", style = RostrumText.meta, color = colors.textSecondary,
                        modifier = Modifier.padding(top = 2.dp))
                }
            }
        }
    }
}
