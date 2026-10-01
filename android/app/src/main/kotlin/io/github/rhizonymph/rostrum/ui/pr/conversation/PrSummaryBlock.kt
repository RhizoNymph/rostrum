package io.github.rhizonymph.rostrum.ui.pr.conversation

import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.LabelChip
import io.github.rhizonymph.rostrum.ui.components.MergeButton
import io.github.rhizonymph.rostrum.ui.components.OutlinedPillButton
import io.github.rhizonymph.rostrum.ui.components.RefTag
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.format.relativeAgo
import io.github.rhizonymph.rostrum.ui.pr.common.dashedBorder
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** Title, refs, author, labels and the merge verdict at the top of the Conversation tab. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PrSummaryBlock(
    header: PullHeader,
    now: Instant,
    onAddLabel: () -> Unit,
    onMerge: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(header.title, style = RostrumText.sheetTitle, color = colors.text)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            RefTag(header.headRef)
            Text("→", style = RostrumText.mono12, color = colors.textSubtle)
            RefTag(header.baseRef)
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            val author = header.author?.login ?: "ghost"
            Avatar(author, size = 24.dp)
            Text(author, style = RostrumText.meta.copy(fontWeight = RostrumText.label.fontWeight), color = colors.text)
            Text("opened ${relativeAgo(header.createdAt, now)}", style = RostrumText.meta, color = colors.textMuted)
        }
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            header.labels.forEach { LabelChip(it) }
            AddLabelButton(onAddLabel)
        }
        MergeVerdictRow(header, onMerge)
    }
}

/** The dashed "+ Label" chip inside a 44dp touch target. */
@Composable
private fun AddLabelButton(onClick: () -> Unit) {
    val colors = RostrumTheme.colors
    Box(
        modifier = Modifier
            .height(44.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics { contentDescription = "Add label" },
        contentAlignment = Alignment.Center,
    ) {
        Row(
            modifier = Modifier
                .height(26.dp)
                .dashedBorder(colors.borderStrong, 6.dp)
                .padding(horizontal = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Icon(RostrumIcons.Plus, contentDescription = null, tint = colors.textMuted, modifier = Modifier.size(12.dp))
            Text("Label", style = RostrumText.chip, color = colors.textMuted)
        }
    }
}

/** Dot, the verdict's sentence, and Merge (enabled only when nothing blocks it). */
@Composable
fun MergeVerdictRow(header: PullHeader, onMerge: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val verdict = header.merge
    Row(
        modifier = modifier.fillMaxWidth().height(48.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        StatusDot(verdict.role.colors().solid, size = 8.dp)
        Text(verdict.sentence, style = RostrumText.meta, color = colors.textSecondary, modifier = Modifier.weight(1f))
        if (header.state == PullState.Open) {
            if (verdict.blocksMerge) {
                OutlinedPillButton("Merge", onClick = {}, enabled = false)
            } else {
                MergeButton("Merge", onClick = onMerge)
            }
        }
    }
}
