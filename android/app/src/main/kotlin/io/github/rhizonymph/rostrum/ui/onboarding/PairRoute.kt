package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel

/**
 * Pairing. [link] is a `rostrum://pair?…` deep link to preview, or `null`
 * for the instructions and manual entry. [onPaired] runs once pairing
 * succeeds (when it also signed the app in, the root has already moved on).
 */
@Composable
fun PairRoute(
    link: String?,
    onBack: () -> Unit,
    onPaired: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = rostrumViewModel(key = "pair:${link.orEmpty()}") { container ->
        PairViewModel(container.backend, container.session, link)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val paired by rememberUpdatedState(onPaired)
    LaunchedEffect(state.paired) {
        if (state.paired) paired()
    }
    PairScreen(state = state, actions = viewModel, onBack = onBack, modifier = modifier)
}
