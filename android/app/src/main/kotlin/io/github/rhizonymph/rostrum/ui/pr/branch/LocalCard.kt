package io.github.rhizonymph.rostrum.ui.pr.branch

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.LocalBranch
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.CopyCommandRow
import io.github.rhizonymph.rostrum.ui.components.DangerOutlinedButton
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.IconTile
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.OutlinedPillButton
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.components.SwitchRow
import io.github.rhizonymph.rostrum.ui.pr.common.CardBody
import io.github.rhizonymph.rostrum.ui.pr.common.CardLabel
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** What the local card can ask of its ViewModel. */
data class LocalActions(
    val onRunOp: (LocalOp) -> Unit,
    val onAbort: () -> Unit,
    val onAutostash: (Boolean) -> Unit,
    val onRetry: () -> Unit,
)

fun opLabel(op: LocalOp): String = when (op) {
    LocalOp.PullRebase -> "Pull (rebase)"
    LocalOp.MergeRemote -> "Merge remote"
    LocalOp.MergeBase -> "Merge base"
    LocalOp.RebaseBase -> "Rebase onto base"
}

/** The paired desktop's view of this branch, and the four local operations. */
@Composable
fun LocalCard(header: PullHeader, state: BranchUiState, actions: LocalActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        CardBody(spacing = 12.dp) {
            when (val local = state.local) {
                LocalCardState.Idle, LocalCardState.Loading -> {
                    CardLabel("Local")
                    LoadingView(label = "Asking the desktop…")
                }
                LocalCardState.NotPaired -> {
                    CardLabel("Local")
                    Text(
                        "Pair your desktop (Settings › Desktop) to see which worktree has this branch and to pull, merge or rebase it there.",
                        style = RostrumText.meta,
                        color = colors.textMuted,
                    )
                }
                is LocalCardState.Failed -> {
                    CardLabel("Local")
                    ErrorView(local.error, title = "Couldn't reach the desktop", onRetry = actions.onRetry)
                }
                is LocalCardState.Ready -> {
                    MachineLine(local.machine)
                    when (val status = local.status) {
                        LocalStatus.NotConfigured -> Text(
                            "There's no clone of ${header.repo} on ${local.machine}. Add one in the desktop's settings to work on this branch locally.",
                            style = RostrumText.meta,
                            color = colors.textMuted,
                        )
                        LocalStatus.NotCheckedOut -> Text(
                            "${header.headRef} isn't checked out in any worktree on ${local.machine}.",
                            style = RostrumText.meta,
                            color = colors.textMuted,
                        )
                        is LocalStatus.CheckedOut -> CheckedOut(status.branch, state, actions)
                    }
                    Text(
                        "Nothing is pushed from here.",
                        style = RostrumText.caption,
                        color = colors.textSubtle,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(top = 2.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun MachineLine(machine: String) {
    val colors = RostrumTheme.colors
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        CardLabel("Local ·")
        Text(machine, style = RostrumText.label.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
        StatusDot(colors.success)
        Text("connected", style = RostrumText.caption, color = colors.textMuted)
    }
}

@Composable
private fun CheckedOut(branch: LocalBranch, state: BranchUiState, actions: LocalActions) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(branch.worktree, style = RostrumText.mono12, color = colors.textMuted)
        val counts = buildList {
            add("↑${branch.ahead} ahead of origin")
            add("↓${branch.behind} behind")
            if (!branch.fetched) add("not fetched")
        }.joinToString(" · ")
        Text(counts, style = RostrumText.mono12, color = colors.textMuted)
    }
    val blocker = branch.blocker
    if (blocker != null && branch.inProgress == null) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.warningText, modifier = Modifier.size(16.dp))
            Text(
                if (state.autostash) "$blocker (will be stashed)" else blocker,
                style = RostrumText.meta,
                color = colors.warningText,
            )
        }
    }
    if (branch.inProgress != null) InProgressBox(branch, state.aborting, actions.onAbort)
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        val blocked = branch.inProgress != null || state.aborting
        LocalOp.entries.chunked(2).forEach { pair ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                pair.forEach { op ->
                    OutlinedPillButton(
                        opLabel(op),
                        onClick = { actions.onRunOp(op) },
                        enabled = !blocked && state.runningOp == null,
                        busy = state.runningOp == op,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
        if (branch.inProgress != null) {
            Text(
                "Available again once the ${branch.inProgress.kind.noun()} finishes or is aborted.",
                style = RostrumText.caption,
                color = colors.textSubtle,
            )
        }
    }
    SwitchRow(
        title = "Stash local changes",
        subtitle = "Passes --autostash; without it a dirty worktree is refused",
        checked = state.autostash,
        onCheckedChange = actions.onAutostash,
        horizontalPadding = 0.dp,
    )
}

@Composable
private fun InProgressBox(branch: LocalBranch, aborting: Boolean, onAbort: () -> Unit) {
    val colors = RostrumTheme.colors
    val inProgress = branch.inProgress ?: return
    val noun = inProgress.kind.noun()
    var confirmAbort by rememberSaveable { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().background(colors.accent.copy(alpha = 0.10f), RoundedCornerShape(12.dp)).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            IconTile(size = 28.dp, radius = 8.dp, color = colors.accent.copy(alpha = 0.18f)) {
                Icon(RostrumIcons.Terminal, contentDescription = null, tint = colors.accentText, modifier = Modifier.size(16.dp))
            }
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(inProgress.description, style = RostrumText.label.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
                val handoff = branch.handoff
                Text(
                    if (handoff?.running == true) {
                        "Handed off to tmux. The worktree is left mid-$noun."
                    } else {
                        "The worktree is left mid-$noun."
                    },
                    style = RostrumText.meta.copy(lineHeight = RostrumText.body.lineHeight),
                    color = colors.textSecondary,
                )
            }
        }
        branch.handoff?.takeIf { it.running }?.let { CopyCommandRow(it.attachCommand, wrap = true) }
        if (inProgress.abortable) {
            DangerOutlinedButton("Abort $noun", onClick = { confirmAbort = true }, busy = aborting)
        }
    }
    if (confirmAbort) {
        ConfirmDialog(
            title = "Abort the $noun?",
            body = "The worktree goes back to how it was before the $noun started. " +
                "Any conflicts resolved so far are lost" +
                (branch.handoff?.takeIf { it.running }?.let { ", and the tmux session ${it.session} is left without work." } ?: "."),
            confirmLabel = "Abort $noun",
            destructive = true,
            onConfirm = {
                confirmAbort = false
                onAbort()
            },
            onDismiss = { confirmAbort = false },
        )
    }
}
