package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.navigation.PrTab

/** The Desktop tab's entry point, wired into the navigation graph. */
@Composable
fun DesktopRoute(
    onOpenPullRequest: (PrRef, PrTab) -> Unit,
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = rostrumViewModel { container -> DesktopViewModel(container.backend, container.session, container.clock) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val now = remember(state) { viewModel.clock.instant() }
    CollectMessages(viewModel.messages.flow)
    DesktopScreen(
        state = state,
        now = now,
        actions = viewModel,
        onOpenPullRequest = onOpenPullRequest,
        onPairDesktop = onPairDesktop,
        modifier = modifier,
    )
}
