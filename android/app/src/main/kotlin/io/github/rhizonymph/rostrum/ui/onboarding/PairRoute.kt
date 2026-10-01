package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel

/**
 * Pairing. [link] is a `rostrum://pair?…` deep link to preview, or `null`
 * for the instructions and manual entry. [onPaired] runs once pairing and
 * its questions are done (when that switched profiles, the root has already
 * moved on to the new profile's graph).
 */
@Composable
fun PairRoute(
    link: String?,
    onBack: () -> Unit,
    onPaired: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = rostrumViewModel(key = "pair:${link.orEmpty()}") { container ->
        PairViewModel(container.profiles, link, container.appMessages)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val paired by rememberUpdatedState(onPaired)
    LaunchedEffect(state.paired) {
        if (state.paired) paired()
    }
    // Back on a question means its "no": stay on the current profile, keep the profile's settings.
    BackHandler(enabled = state.switchOffer != null) { viewModel.stayOnCurrent() }
    BackHandler(enabled = state.copy != null) { viewModel.keepPhoneSettings() }
    PairScreen(state = state, actions = viewModel, onBack = onBack, modifier = modifier)
}
