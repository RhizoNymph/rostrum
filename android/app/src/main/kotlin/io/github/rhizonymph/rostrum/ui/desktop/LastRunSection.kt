package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.format.relativeAgo
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** The last finished "sync all": its summary, what needs a look, and what updated. */
@Composable
fun LastRunSection(
    run: LastRunView,
    now: Instant,
    onOpenPullRequest: (PrRef) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader("Last run")
        RostrumCard(Modifier.fillMaxWidth()) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(run.title, style = RostrumText.label.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
                    Text(run.summary, style = RostrumText.meta, color = colors.textMuted)
                }
                run.finishedAt?.let {
                    Text(relativeAgo(it, now), style = RostrumText.mono12, color = colors.textSubtle, modifier = Modifier.padding(top = 2.dp))
                }
            }
            run.attention.forEach { entry ->
                CardDivider()
                AttentionRow(entry, onClick = { onOpenPullRequest(entry.pr) })
            }
            if (run.settled.isNotEmpty()) {
                CardDivider()
                val state = if (expanded) "expanded" else "collapsed"
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clickable(role = Role.Button, onClick = { expanded = !expanded })
                        .semantics { stateDescription = state }
                        .padding(start = 16.dp, end = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    StatusChip("${run.updatedCount} updated", ColorRole.Success)
                    Spacer(Modifier.weight(1f))
                    Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                        Icon(
                            if (expanded) RostrumIcons.ChevronUp else RostrumIcons.ChevronDown,
                            contentDescription = null,
                            tint = colors.textMuted,
                            modifier = Modifier.size(18.dp),
                        )
                    }
                }
                if (expanded) {
                    run.settled.forEach { entry ->
                        SettledRow(entry, onClick = { onOpenPullRequest(entry.pr) })
                    }
                }
            }
        }
        Text(
            "Nothing is ever pushed. After a merge or rebase, the ahead count is your cue to push from the desktop.",
            style = RostrumText.caption,
            color = colors.textSubtle,
            modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 12.dp),
        )
    }
}

@Composable
private fun AttentionRow(entry: SyncEntryView, onClick: () -> Unit) {
    val colors = RostrumTheme.colors
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("#${entry.pr.number}", style = RostrumText.mono12, color = colors.textMuted)
            Text(
                entry.headRef,
                style = RostrumText.mono13,
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            entry.chip?.let { StatusChip(it) }
        }
        Text(entry.detail, style = RostrumText.caption, color = colors.textMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun SettledRow(entry: SyncEntryView, onClick: () -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("#${entry.pr.number}", style = RostrumText.mono12, color = colors.textMuted)
        Text(
            entry.headRef,
            style = RostrumText.mono13,
            color = colors.textSecondary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Text(entry.detail, style = RostrumText.caption, color = colors.textSubtle, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

