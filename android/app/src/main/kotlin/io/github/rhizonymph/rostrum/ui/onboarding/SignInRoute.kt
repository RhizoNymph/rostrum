package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.ui.common.LocalProfileHandle
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel

/**
 * The sign-in screen: on first run, or for the active profile once it lost
 * its token. Signing in needs no navigation from here: the root switches to
 * the profile's feed. [onOpenProfiles] (when other profiles exist) opens the
 * switcher.
 */
@Composable
fun SignInRoute(
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
    onOpenProfiles: (() -> Unit)? = null,
) {
    val profile = LocalProfileHandle.current
    val viewModel = rostrumViewModel { container -> SignInViewModel(container.profiles, profile) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    SignInScreen(
        state = state,
        actions = viewModel,
        onPairDesktop = onPairDesktop,
        modifier = modifier,
        onOpenProfiles = onOpenProfiles,
    )
}
