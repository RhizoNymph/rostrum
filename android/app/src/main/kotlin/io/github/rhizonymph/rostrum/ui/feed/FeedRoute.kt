package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.data.model.PrRef

/** Placeholder until this screen is built. */
@Composable
fun FeedRoute(
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    EmptyView(title = "Feed", modifier = modifier)
}
