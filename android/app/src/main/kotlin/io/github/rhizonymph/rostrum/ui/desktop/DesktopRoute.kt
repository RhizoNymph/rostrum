package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.desktopconfig.PushSettingsSheet
import io.github.rhizonymph.rostrum.ui.desktopconfig.PushSettingsViewModel
import io.github.rhizonymph.rostrum.ui.navigation.PrTab

/** The Desktop tab's entry point, wired into the navigation graph. */
@Composable
fun DesktopRoute(
    profileLabel: String,
    onOpenPullRequest: (PrRef, PrTab) -> Unit,
    onPairDesktop: () -> Unit,
    onOpenProfiles: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = profileViewModel { container, profile -> DesktopViewModel(profile.backend, profile.session, container.clock) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val now = remember(state) { viewModel.clock.instant() }
    CollectMessages(viewModel.messages.flow)
    val pushSheet = profileViewModel { _, profile -> PushSettingsViewModel(profile.backend) }
    val pushState by pushSheet.state.collectAsStateWithLifecycle()
    CollectMessages(pushSheet.messages.flow)
    DesktopScreen(
        state = state,
        now = now,
        actions = viewModel,
        onOpenPullRequest = onOpenPullRequest,
        onPairDesktop = onPairDesktop,
        modifier = modifier,
        profileLabel = profileLabel,
        onOpenProfiles = onOpenProfiles,
        onSendSettings = pushSheet::open,
    )
    val machine = ((state.page as? DesktopPage.Loaded)?.content?.machine?.name)
    PushSettingsSheet(pushState, pushSheet, machine)
}
