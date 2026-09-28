package io.github.rhizonymph.rostrum.ui.settings

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.ui.components.EmptyView

/** Placeholder until this screen is built. */
@Composable
fun SettingsRoute(
    onPairDesktop: () -> Unit,
    onOpenDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    EmptyView(title = "Settings", modifier = modifier)
}
