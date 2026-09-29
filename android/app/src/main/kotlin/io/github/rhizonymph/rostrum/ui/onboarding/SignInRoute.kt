package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel

/**
 * The first-run screen. Signing in needs no navigation from here: the session
 * becomes signed in and the root switches to the feed.
 */
@Composable
fun SignInRoute(
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = rostrumViewModel { container -> SignInViewModel(container.session) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    SignInScreen(state = state, actions = viewModel, onPairDesktop = onPairDesktop, modifier = modifier)
}
