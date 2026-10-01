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
import io.github.rhizonymph.rostrum.ui.items.RowCallbacks
import io.github.rhizonymph.rostrum.ui.stacks.StackActionsHost
import io.github.rhizonymph.rostrum.ui.stacks.StackActionsViewModel

/** One repository of the active profile, wired into the navigation graph; Back returns to the feed. */
@Composable
fun RepoRoute(
    repo: String,
    onBack: () -> Unit,
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenIssue: (IssueRef) -> Unit,
    onNewIssue: (repo: String) -> Unit,
    modifier: Modifier = Modifier,
    onPairDesktop: () -> Unit = {},
) {
    val viewModel = profileViewModel(key = "repo:$repo") { container, profile ->
        RepoViewModel(profile.backend, repo, container.clock)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    CollectMessages(viewModel.messages.flow)
    val stacks = profileViewModel(key = "repo-stacks:$repo") { _, profile -> StackActionsViewModel(profile.backend, profile.session.state) }
    val stackFlow by stacks.flow.collectAsStateWithLifecycle()
    val selection by stacks.selection.collectAsStateWithLifecycle()
    CollectMessages(stacks.messages.flow)
    val openPulls = state.overview.dataOrNull()?.pulls?.flatMap { it.pulls }.orEmpty()
    val picked = selection?.takeIf { it.repo == repo }?.picked
    BackHandler(enabled = picked != null) { stacks.cancelArrange() }
    RepoScreen(
        repo = repo,
        state = state,
        actions = viewModel,
        onBack = onBack,
        onOpenPullRequest = onOpenPullRequest,
        onOpenIssue = onOpenIssue,
        onNewIssue = { onNewIssue(repo) },
        modifier = modifier,
        rows = RowCallbacks(
            openPullRequest = onOpenPullRequest,
            openIssue = onOpenIssue,
            stackAction = { header, entry -> stacks.request(entry, header.repo, header.stack, header.members) },
            picked = picked,
            togglePicked = stacks::toggleSelected,
        ),
        arrange = ArrangeControls(
            picked = picked,
            start = { stacks.startArrange(repo) },
            cancel = stacks::cancelArrange,
            next = { stacks.arrangeNext(openPulls) },
        ),
    )
    StackActionsHost(stackFlow, stacks, onPairDesktop)
    state.trunkEditor?.let { editor ->
        BackHandler { viewModel.closeTrunkEditor() }
        RostrumBottomSheet(onDismiss = viewModel::closeTrunkEditor) {
            TrunkEditorContent(editor, state.branches?.dataOrNull()?.trunks?.existing.orEmpty(), viewModel)
        }
    }
}
