package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.ui.components.EmptyView

/** Placeholder until this screen is built. */
@Composable
fun PairRoute(
    link: String?,
    onBack: () -> Unit,
    onPaired: () -> Unit,
    modifier: Modifier = Modifier,
) {
    EmptyView(title = "Pair", modifier = modifier)
}
