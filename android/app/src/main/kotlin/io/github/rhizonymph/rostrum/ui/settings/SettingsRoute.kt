package io.github.rhizonymph.rostrum.ui.settings

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.notifications.NotificationPermissionState
import io.github.rhizonymph.rostrum.notifications.rememberNotificationPermission
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.profileViewModel

/**
 * The Settings tab's entry point, wired into the navigation graph. Everything
 * here belongs to the active profile, except [profilesSection] (the list of
 * profiles, from the profiles feature), shown first.
 */
@Composable
fun SettingsRoute(
    onPairDesktop: () -> Unit,
    onOpenDesktop: () -> Unit,
    modifier: Modifier = Modifier,
    profilesSection: @Composable () -> Unit = {},
) {
    val viewModel = profileViewModel { container, profile ->
        SettingsViewModel(profile.backend, profile.session) { container.onNotificationSettingsChanged() }
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val permission = rememberNotificationPermission()
    val actions = remember(viewModel, permission) { PermissionAwareActions(viewModel, permission) }
    val copySheet = profileViewModel { _, profile -> DesktopConfigSheetViewModel(profile.backend) }
    val sheetState by copySheet.state.collectAsStateWithLifecycle()
    CollectMessages(viewModel.messages.flow)
    CollectMessages(copySheet.messages.flow)
    LaunchedEffect(copySheet) { copySheet.copied.collect { viewModel.refreshContent() } }
    // Back from pairing (which may have copied the desktop's settings) or another tab: reload.
    var resumedOnce by remember { mutableStateOf(false) }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        if (resumedOnce) viewModel.refreshContent() else resumedOnce = true
    }
    SettingsScreen(
        state = state,
        actions = actions,
        onOpenDesktop = onOpenDesktop,
        onPairDesktop = onPairDesktop,
        onCopySettings = copySheet::open,
        modifier = modifier,
        profilesSection = profilesSection,
    )
    val machine = ((state.content as? UiState.Loaded)?.data?.desktop as? DesktopSummary.Connected)?.machine?.name
    CopySettingsSheet(sheetState, copySheet, machine)
}

/** Turning a notification toggle on asks for the permission it needs, when missing. */
private class PermissionAwareActions(
    private val delegate: SettingsActions,
    private val permission: NotificationPermissionState,
) : SettingsActions by delegate {
    override fun setNotifyNewPullRequests(enabled: Boolean) {
        if (enabled && !permission.granted) permission.request()
        delegate.setNotifyNewPullRequests(enabled)
    }

    override fun setNotifyReviewRequests(enabled: Boolean) {
        if (enabled && !permission.granted) permission.request()
        delegate.setNotifyReviewRequests(enabled)
    }
}
