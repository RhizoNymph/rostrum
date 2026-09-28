package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncEntryState
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.SwitchRow
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The three "sync all" operations, the stash switch, and a running run's progress. */
@Composable
fun SyncAllSection(
    sync: SyncActivity,
    autostash: Boolean,
    onStart: (SyncAllOp) -> Unit,
    onAutostashChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val idle = sync == SyncActivity.Idle
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader("Sync all worktrees")
        RostrumCard(Modifier.fillMaxWidth()) {
            SyncAllOp.entries.forEach { op ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 52.dp)
                        .clickable(enabled = idle, role = Role.Button, onClick = { onStart(op) })
                        .padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Text(
                        op.title,
                        style = RostrumText.rowTitle,
                        color = if (idle) colors.text else colors.textSubtle,
                        modifier = Modifier.weight(1f),
                    )
                    if (sync is SyncActivity.Starting && sync.op == op) {
                        CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
                    }
                    Text(op.hint, style = RostrumText.mono11, color = colors.textSubtle)
                }
                CardDivider()
            }
            SwitchRow(
                title = "Stash local changes",
                subtitle = "Passes --autostash; without it a dirty worktree is refused",
                checked = autostash,
                onCheckedChange = onAutostashChange,
                enabled = idle,
            )
            if (sync is SyncActivity.Running) {
                CardDivider()
                RunningProgress(sync)
            }
        }
    }
}

@Composable
private fun RunningProgress(sync: SyncActivity.Running) {
    val colors = RostrumTheme.colors
    val run = sync.run
    val total = run.summary.total.coerceAtLeast(1)
    Column(
        Modifier
            .fillMaxWidth()
            .padding(16.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(runTitle(run.op), style = RostrumText.label, color = colors.text, modifier = Modifier.weight(1f))
            Text(run.progressText, style = RostrumText.mono12, color = colors.textMuted)
        }
        LinearProgressIndicator(
            progress = { run.summary.done.toFloat() / total },
            modifier = Modifier.fillMaxWidth(),
            color = colors.accent,
            trackColor = colors.border,
        )
        run.entries.filter { it.state == SyncEntryState.Running }.forEach { entry ->
            Text(
                "#${entry.number} ${entry.headRef}",
                style = RostrumText.mono12,
                color = colors.textMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
