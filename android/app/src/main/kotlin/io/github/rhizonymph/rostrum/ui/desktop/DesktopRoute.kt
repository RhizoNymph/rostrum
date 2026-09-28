package io.github.rhizonymph.rostrum.ui.desktop

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.navigation.PrTab

/** Placeholder until this screen is built. */
@Composable
fun DesktopRoute(
    onOpenPullRequest: (PrRef, PrTab) -> Unit,
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    EmptyView(title = "Desktop", modifier = modifier)
}
