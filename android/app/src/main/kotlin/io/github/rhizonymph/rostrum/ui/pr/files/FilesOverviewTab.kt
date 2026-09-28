package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.FilesOverview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The two views of the Files tab; Diff opens the largest file's diff. */
private enum class FilesView(val label: String) { Overview("Overview"), Diff("Diff") }

/** The PR screen's Files tab: the overview of where the change is. */
@Composable
fun FilesOverviewTab(
    pr: PrRef,
    onOpenFile: (fileIndex: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = profileViewModel(key = "files-overview-$pr") { _, profile -> FilesOverviewViewModel(profile.backend, pr) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    // Coming back from a diff: counts of drafts and threads may have changed.
    LifecycleResumeEffect(viewModel) {
        viewModel.refresh()
        onPauseOrDispose { }
    }
    FilesOverviewContent(
        state = state,
        onOpenFile = onOpenFile,
        onShowDiff = { viewModel.firstRankedFile()?.let(onOpenFile) },
        onRetry = viewModel::retry,
        modifier = modifier,
    )
}

@Composable
fun FilesOverviewContent(
    state: UiState<FilesOverview>,
    onOpenFile: (Int) -> Unit,
    onShowDiff: () -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    LazyColumn(
        modifier = modifier.fillMaxSize().background(colors.bg),
        contentPadding = PaddingValues(bottom = 24.dp),
    ) {
        item(key = "toggle") {
            SegmentedToggle(
                options = FilesView.entries,
                selected = FilesView.Overview,
                onSelect = { if (it == FilesView.Diff) onShowDiff() },
                label = { it.label },
                modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 8.dp),
            )
        }
        when (state) {
            UiState.Loading -> item(key = "loading") { LoadingView(label = "Loading changed files…") }
            is UiState.Error -> item(key = "error") {
                ErrorView(state.error, Modifier.padding(16.dp), title = "Couldn't load the changed files", onRetry = onRetry)
            }
            is UiState.Loaded -> {
                val overview = state.data
                if (overview.files.isEmpty()) {
                    item(key = "empty") { EmptyView("No changed files", body = "This pull request doesn't change any files.") }
                } else {
                    item(key = "summary") {
                        SummaryStrip(overview.stats, Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 12.dp))
                    }
                    item(key = "map") {
                        ChangeMapCard(overview, onOpenFile, Modifier.padding(horizontal = 16.dp))
                    }
                    item(key = "legend") {
                        Text(
                            "Width: directory share · Height: file share · Colour: added vs removed",
                            style = RostrumText.caption,
                            color = colors.textMuted,
                            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp),
                        )
                    }
                    item(key = "ranked-title") {
                        Text(
                            "Largest changes",
                            style = RostrumText.section,
                            color = colors.textMuted,
                            modifier = Modifier
                                .padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 8.dp)
                                .semantics { heading() },
                        )
                    }
                    item(key = "ranked") {
                        RankedFilesCard(overview.ranked, onOpenFile, Modifier.padding(horizontal = 16.dp))
                    }
                }
            }
        }
    }
}

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun FilesOverviewPreview() {
    val overview = PreviewData.sample { filesOverview(PrRef("RhizoNymph/rostrum", 10)) }
    RostrumTheme {
        FilesOverviewContent(UiState.Loaded(overview), onOpenFile = {}, onShowDiff = {}, onRetry = {})
    }
}

@Preview(widthDp = 412, heightDp = 400)
@Composable
private fun FilesOverviewLoadingPreview() {
    RostrumTheme { FilesOverviewContent(UiState.Loading, onOpenFile = {}, onShowDiff = {}, onRetry = {}) }
}
