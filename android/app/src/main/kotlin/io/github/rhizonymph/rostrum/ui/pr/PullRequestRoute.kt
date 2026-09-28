package io.github.rhizonymph.rostrum.ui.pr

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.navigation.PrTab

/** Placeholder until this screen is built. */
@Composable
fun PullRequestRoute(
    pr: PrRef,
    initialTab: PrTab,
    onBack: () -> Unit,
    filesTab: @Composable (Modifier) -> Unit,
    reviewSheet: @Composable (onDismiss: () -> Unit, onSubmitted: () -> Unit) -> Unit,
    modifier: Modifier = Modifier,
) {
    EmptyView(title = "Pull request", modifier = modifier)
}
