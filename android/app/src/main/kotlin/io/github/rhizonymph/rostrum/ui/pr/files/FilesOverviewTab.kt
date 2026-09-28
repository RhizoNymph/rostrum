package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.data.model.PrRef

/** Placeholder until this screen is built. */
@Composable
fun FilesOverviewTab(
    pr: PrRef,
    onOpenFile: (fileIndex: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    EmptyView(title = "Files", modifier = modifier)
}
