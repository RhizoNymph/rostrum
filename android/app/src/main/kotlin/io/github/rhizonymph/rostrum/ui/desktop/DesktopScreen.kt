package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.HeaderPill
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.ScreenHeader
import io.github.rhizonymph.rostrum.ui.navigation.PrTab
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/**
 * The Desktop tab, stateless. [now] anchors the relative times; the header
 * pill names the active profile ([profileLabel]) and opens the switcher.
 */
@Composable
fun DesktopScreen(
    state: DesktopState,
    now: Instant,
    actions: DesktopActions,
    onOpenPullRequest: (PrRef, PrTab) -> Unit,
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
    profileLabel: String? = null,
    onOpenProfiles: () -> Unit = {},
) {
    val colors = RostrumTheme.colors
    var confirmUnpair by rememberSaveable { mutableStateOf(false) }
    val page = state.page
    val machineName = (page as? DesktopPage.Loaded)?.content?.machine?.name ?: "the desktop"
    Column(modifier.fillMaxSize().background(colors.bg)) {
        ScreenHeader("Desktop") {
            if (profileLabel != null) {
                HeaderPill(
                    label = profileLabel,
                    description = "Profile $profileLabel. Switch profiles",
                    onClick = onOpenProfiles,
                    modifier = Modifier.padding(end = 2.dp),
                )
            }
            if (page is DesktopPage.Loaded || page is DesktopPage.Unreachable) {
                DesktopMenu(
                    onRefresh = actions::refresh,
                    onFetchToken = actions::fetchGitHubToken,
                    onUnpair = { confirmUnpair = true },
                )
            }
        }
        when (page) {
            DesktopPage.Loading -> LoadingView(label = "Asking the desktop")
            DesktopPage.NotPaired -> EmptyView(
                title = "No desktop paired",
                body = "Pair with the desktop running Rostrum to see which worktree each pull request is checked out in, " +
                    "pull, merge and rebase them, and pick up conflicts handed to tmux.",
                action = { PrimaryButton("Pair with your desktop", onPairDesktop) },
            )
            is DesktopPage.Unreachable -> ErrorView(
                page.error,
                modifier = Modifier.padding(horizontal = 12.dp),
                title = "Couldn't reach the desktop",
                onRetry = actions::refresh,
            )
            is DesktopPage.Loaded -> LoadedDesktop(page.content, now, actions, onOpenPullRequest)
        }
    }
    if (confirmUnpair) {
        ConfirmDialog(
            title = "Unpair $machineName?",
            body = "This phone forgets the desktop and the desktop forgets this phone. Local operations stop until you pair again.",
            confirmLabel = "Unpair",
            destructive = true,
            onConfirm = {
                confirmUnpair = false
                actions.unpair()
            },
            onDismiss = { confirmUnpair = false },
        )
    }
    state.unpairFailure?.let { error ->
        ConfirmDialog(
            title = "Couldn't reach $machineName",
            body = "${error.describe()} Forget it on this phone anyway? The desktop keeps listing this phone until you remove it there.",
            confirmLabel = "Forget on this phone",
            destructive = true,
            onConfirm = actions::forgetDesktop,
            onDismiss = actions::dismissUnpairFailure,
        )
    }
}

@Composable
private fun DesktopMenu(onRefresh: () -> Unit, onFetchToken: () -> Unit, onUnpair: () -> Unit) {
    val colors = RostrumTheme.colors
    var open by remember { mutableStateOf(false) }
    Box {
        RostrumIconButton(RostrumIcons.MoreVert, "Desktop options", onClick = { open = true }, iconSize = 20.dp)
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = colors.raised) {
            DropdownMenuItem(
                text = { Text("Refresh", style = RostrumText.label, color = colors.text) },
                onClick = { open = false; onRefresh() },
            )
            DropdownMenuItem(
                text = { Text("Get GitHub token from desktop", style = RostrumText.label, color = colors.text) },
                onClick = { open = false; onFetchToken() },
            )
            DropdownMenuItem(
                text = { Text("Unpair…", style = RostrumText.label, color = colors.dangerText) },
                onClick = { open = false; onUnpair() },
            )
        }
    }
}

@Composable
private fun LoadedDesktop(
    content: DesktopContent,
    now: Instant,
    actions: DesktopActions,
    onOpenPullRequest: (PrRef, PrTab) -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 12.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        MachineCard(content.machine)
        HandoffSection(
            handoffs = content.handoffs,
            aborting = content.aborting,
            now = now,
            onOpenPullRequest = { session -> session.pr?.let { onOpenPullRequest(it, PrTab.Branch) } },
            onAbort = actions::abort,
            onCopied = actions::onCopied,
            onRetry = actions::refresh,
        )
        SyncAllSection(
            sync = content.sync,
            autostash = content.autostash,
            onStart = actions::startSync,
            onAutostashChange = actions::setAutostash,
        )
        content.lastRun?.let { run ->
            LastRunSection(lastRunView(run), now, onOpenPullRequest = { pr -> onOpenPullRequest(pr, PrTab.Branch) })
        }
    }
}

// --- previews ----------------------------------------------------------------

@Preview(widthDp = 412, heightDp = 1400)
@Composable
private fun DesktopPreview() {
    val machine = PreviewData.sample { machineInfo() }
    val handoffs = PreviewData.sample { handoffs() }
    val lastRun = PreviewData.sample { syncAllStatus() }
    RostrumTheme {
        DesktopScreen(
            state = DesktopState(
                page = DesktopPage.Loaded(
                    DesktopContent(machine, UiState.Loaded(handoffs), autostash = false, sync = SyncActivity.Idle, lastRun = lastRun),
                ),
            ),
            now = PreviewData.clock.instant(),
            actions = NoDesktopActions,
            onOpenPullRequest = { _, _ -> },
            onPairDesktop = {},
        )
    }
}

@Preview(widthDp = 412, heightDp = 600)
@Composable
private fun DesktopNotPairedPreview() {
    RostrumTheme {
        DesktopScreen(DesktopState(DesktopPage.NotPaired), Instant.now(), NoDesktopActions, { _, _ -> }, {})
    }
}

@Preview(widthDp = 412, heightDp = 600)
@Composable
private fun DesktopUnreachablePreview() {
    RostrumTheme {
        DesktopScreen(
            DesktopState(DesktopPage.Unreachable(BackendError.DesktopUnreachable("no route to 192.168.1.24:8485"))),
            Instant.now(), NoDesktopActions, { _, _ -> }, {},
        )
    }
}
