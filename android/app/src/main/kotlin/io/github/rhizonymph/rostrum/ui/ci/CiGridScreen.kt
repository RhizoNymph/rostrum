package io.github.rhizonymph.rostrum.ui.ci

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleStartEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FilterPill
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.TitleStack
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The CI grid of the active profile, wired into the navigation graph:
 * every repository's, or [repo]'s alone. It ticks and polls only while
 * started, so a hidden screen costs nothing.
 */
@Composable
fun CiRoute(
    repo: String?,
    onBack: () -> Unit,
    onOpenPullRequest: (PrRef) -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = profileViewModel(key = "ci:${repo.orEmpty()}") { _, profile -> CiGridViewModel(profile.backend, repo) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    CollectMessages(viewModel.messages.flow)
    LifecycleStartEffect(viewModel) {
        viewModel.start()
        onStopOrDispose { viewModel.stop() }
    }
    CiGridScreen(
        repo = repo,
        state = state,
        actions = viewModel,
        onBack = onBack,
        onOpenPullRequest = onOpenPullRequest,
        modifier = modifier,
    )
    state.sheet?.let { CiCellSheet(it, viewModel) }
}

/** The CI grid, stateless. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CiGridScreen(
    repo: String?,
    state: CiGridUiState,
    actions: CiGridActions,
    onBack: () -> Unit,
    onOpenPullRequest: (PrRef) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxSize().background(colors.bg)) {
        BackTopBar(onBack = onBack) {
            TitleStack(repo ?: "Every repository") {
                Text("Checks", style = RostrumText.sheetTitle, color = colors.text, maxLines = 1)
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FilterPill("Needs attention", state.filter.needsAttention, { actions.setNeedsAttention(!state.filter.needsAttention) })
        }
        PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = actions::refresh, modifier = Modifier.weight(1f).fillMaxWidth()) {
            when (val content = state.content) {
                UiState.Loading -> LoadingView(label = "Reading checks")
                is UiState.Error -> ErrorView(content.error, Modifier.padding(12.dp), title = "Couldn't show the checks", onRetry = actions::refresh)
                is UiState.Loaded -> if (content.data.sections.isEmpty()) {
                    Box(Modifier.fillMaxSize().padding(24.dp)) {
                        Text("No repositories to show", style = RostrumText.body, color = colors.textMuted)
                    }
                } else {
                    CiGridTable(
                        grid = content.data,
                        filter = state.filter,
                        onOpenCell = actions::openCell,
                        onOpenPullRequest = { r, number -> onOpenPullRequest(PrRef(r, number)) },
                    )
                }
            }
        }
    }
}

@Preview(widthDp = 412, heightDp = 800)
@Composable
private fun CiGridPreview() {
    val grid = io.github.rhizonymph.rostrum.ui.preview.PreviewData.sample { refreshCi(io.github.rhizonymph.rostrum.data.model.CiGridFilter()) }
    RostrumTheme {
        CiGridScreen(
            repo = null,
            state = CiGridUiState(content = UiState.Loaded(grid)),
            actions = NoCiGridActions,
            onBack = {},
            onOpenPullRequest = {},
        )
    }
}
