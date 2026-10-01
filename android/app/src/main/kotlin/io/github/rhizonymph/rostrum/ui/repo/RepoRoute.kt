package io.github.rhizonymph.rostrum.ui.repo

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet

/** One repository of the active profile, wired into the navigation graph; Back returns to the feed. */
@Composable
fun RepoRoute(
    repo: String,
    onBack: () -> Unit,
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenIssue: (IssueRef) -> Unit,
    onNewIssue: (repo: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = profileViewModel(key = "repo:$repo") { container, profile ->
        RepoViewModel(profile.backend, repo, container.clock)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    CollectMessages(viewModel.messages.flow)
    RepoScreen(
        repo = repo,
        state = state,
        actions = viewModel,
        onBack = onBack,
        onOpenPullRequest = onOpenPullRequest,
        onOpenIssue = onOpenIssue,
        onNewIssue = { onNewIssue(repo) },
        modifier = modifier,
    )
    state.trunkEditor?.let { editor ->
        BackHandler { viewModel.closeTrunkEditor() }
        RostrumBottomSheet(onDismiss = viewModel::closeTrunkEditor) {
            TrunkEditorContent(editor, state.branches?.dataOrNull()?.trunks?.existing.orEmpty(), viewModel)
        }
    }
}
