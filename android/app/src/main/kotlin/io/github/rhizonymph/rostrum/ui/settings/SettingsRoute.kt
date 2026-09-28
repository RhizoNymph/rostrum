package io.github.rhizonymph.rostrum.ui.settings

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.notifications.NotificationPermissionState
import io.github.rhizonymph.rostrum.notifications.rememberNotificationPermission
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel

/** The Settings tab's entry point, wired into the navigation graph. */
@Composable
fun SettingsRoute(
    onPairDesktop: () -> Unit,
    onOpenDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = rostrumViewModel { container ->
        SettingsViewModel(container.backend, container.session, container::onNotificationSettingsChanged)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val permission = rememberNotificationPermission()
    val actions = remember(viewModel, permission) { PermissionAwareActions(viewModel, permission) }
    CollectMessages(viewModel.messages.flow)
    SettingsScreen(
        state = state,
        actions = actions,
        onOpenDesktop = onOpenDesktop,
        onPairDesktop = onPairDesktop,
        modifier = modifier,
    )
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
