package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.CopyCommandRow
import io.github.rhizonymph.rostrum.ui.components.DangerOutlinedButton
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.IconTile
import io.github.rhizonymph.rostrum.ui.components.OutlinedPillButton
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** The machine card: tile, name, connection line and version. */
@Composable
fun MachineCard(machine: MachineInfo, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        Row(
            Modifier.padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            IconTile(size = 44.dp, radius = 12.dp) {
                Icon(RostrumIcons.Desktop, contentDescription = null, tint = colors.accentText, modifier = Modifier.size(22.dp))
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(machine.name, style = RostrumText.cardTitle, color = colors.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    StatusDot(colors.success)
                    Text(machineSummary(machine), style = RostrumText.meta, color = colors.textMuted)
                }
                Text("rostrum ${machine.version}", style = RostrumText.mono12, color = colors.textSubtle)
            }
        }
    }
}

/** Sessions holding stopped rebases or merges, each with its attach command. */
@Composable
fun HandoffSection(
    handoffs: UiState<List<HandoffSession>>,
    aborting: Set<String>,
    now: Instant,
    onOpenPullRequest: (HandoffSession) -> Unit,
    onAbort: (HandoffSession) -> Unit,
    onCopied: () -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val count = (handoffs as? UiState.Loaded)?.data?.size ?: 0
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader(
            "Handoff sessions",
            trailing = if (count > 0) waitingLabel(count) else null,
            trailingColor = colors.warningText,
        )
        when (handoffs) {
            UiState.Loading -> Unit
            is UiState.Error -> ErrorView(handoffs.error, title = "Couldn't list handoff sessions", onRetry = onRetry)
            is UiState.Loaded -> if (handoffs.data.isEmpty()) {
                RostrumCard(Modifier.fillMaxWidth()) {
                    Text(
                        "No stopped rebases or merges",
                        style = RostrumText.meta,
                        color = colors.textMuted,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            } else {
                handoffs.data.forEach { session ->
                    HandoffCard(
                        session = session,
                        aborting = session.session in aborting,
                        now = now,
                        onOpenPullRequest = { onOpenPullRequest(session) },
                        onAbort = { onAbort(session) },
                        onCopied = onCopied,
                    )
                }
            }
        }
    }
}

@Composable
private fun HandoffCard(
    session: HandoffSession,
    aborting: Boolean,
    now: Instant,
    onOpenPullRequest: () -> Unit,
    onAbort: () -> Unit,
    onCopied: () -> Unit,
) {
    val colors = RostrumTheme.colors
    var confirmAbort by rememberSaveable(session.session) { mutableStateOf(false) }
    RostrumCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                IconTile(size = 36.dp, color = colors.accent.copy(alpha = 0.15f)) {
                    Icon(RostrumIcons.Terminal, contentDescription = null, tint = colors.accentText, modifier = Modifier.size(20.dp))
                }
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(
                        session.session,
                        style = RostrumText.mono13.copy(fontWeight = FontWeight.Medium),
                        color = colors.text,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    session.worktree?.let {
                        Text(it, style = RostrumText.mono12, color = colors.textSecondary, maxLines = 1, overflow = TextOverflow.StartEllipsis)
                    }
                    val meta = handoffMeta(session, now)
                    if (meta.isNotEmpty()) Text(meta, style = RostrumText.caption, color = colors.textSubtle)
                }
            }
            CopyCommandRow(session.attachCommand, onCopied = onCopied)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedPillButton(
                    "Open PR",
                    onOpenPullRequest,
                    modifier = Modifier.weight(1f),
                    enabled = session.pr != null,
                )
                if (session.pr != null) {
                    DangerOutlinedButton(
                        "Abort",
                        { confirmAbort = true },
                        modifier = Modifier.weight(1f),
                        busy = aborting,
                    )
                }
            }
        }
    }
    if (confirmAbort) {
        ConfirmDialog(
            title = "Abort the stopped operation?",
            body = "The rebase or merge stopped in ${session.worktree ?: session.headRef ?: "this worktree"} on the desktop " +
                "is aborted and the worktree goes back to how it was before it started. The tmux session is left for you to close.",
            confirmLabel = "Abort",
            destructive = true,
            onConfirm = {
                confirmAbort = false
                onAbort()
            },
            onDismiss = { confirmAbort = false },
        )
    }
}
