package io.github.rhizonymph.rostrum.ui.pr

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.navigation.PrTab
import io.github.rhizonymph.rostrum.ui.pr.branch.BranchTab
import io.github.rhizonymph.rostrum.ui.pr.branch.BranchUiState
import io.github.rhizonymph.rostrum.ui.pr.branch.LocalActions
import io.github.rhizonymph.rostrum.ui.pr.checks.ChecksTab
import io.github.rhizonymph.rostrum.ui.pr.common.ComposerBar
import io.github.rhizonymph.rostrum.ui.pr.conversation.ConversationTab
import io.github.rhizonymph.rostrum.ui.pr.conversation.ReplyActions
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** Every callback the pull request screen raises. */
data class PrScreenActions(
    val onBack: () -> Unit,
    val onTab: (PrTab) -> Unit,
    val onRetry: () -> Unit,
    val menu: PrMenuActions,
    val onCommentText: (String) -> Unit,
    val onSendComment: () -> Unit,
    val onReview: () -> Unit,
    val reply: ReplyActions,
    val onAddLabel: () -> Unit,
    val onMerge: () -> Unit,
    val onUpdateBranch: (BranchUpdateMethod) -> Unit,
    val local: LocalActions,
)

/**
 * The pull request screen, stateless: header, tab row, the selected tab, and
 * the composer under Conversation and Checks. [filesTab] is the Files tab,
 * supplied by the navigation graph.
 */
@Composable
fun PrScreen(
    state: PrDetailUiState,
    branch: BranchUiState,
    tab: PrTab,
    now: Instant,
    actions: PrScreenActions,
    filesTab: @Composable (Modifier) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val detail = state.detail.dataOrNull()
    Column(modifier.fillMaxSize().background(colors.bg)) {
        PrTopBar(state.pr, detail?.header, state.busy, actions.onBack, actions.menu)
        PrTabRow(tab, detail?.header?.changedFiles, actions.onTab)
        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (tab == PrTab.Files) {
                filesTab(Modifier.fillMaxSize())
            } else {
                DetailBody(state.detail, actions.onRetry) { loaded ->
                    TabBody(tab, loaded, state, branch, now, actions)
                }
            }
            if (state.refreshing && detail != null && tab != PrTab.Files) {
                LinearProgressIndicator(
                    color = colors.accent,
                    trackColor = colors.bg,
                    modifier = Modifier.fillMaxWidth().height(2.dp).align(Alignment.TopCenter),
                )
            }
        }
        if (detail != null && (tab == PrTab.Conversation || tab == PrTab.Checks)) {
            ComposerBar(
                text = state.comment,
                onTextChange = actions.onCommentText,
                onSend = actions.onSendComment,
                sending = state.postingComment,
                pendingDrafts = detail.pendingReview.drafts.size,
                onReview = actions.onReview,
                modifier = Modifier.imePadding(),
            )
        }
    }
}

@Composable
private fun DetailBody(
    detail: UiState<PullDetail>,
    onRetry: () -> Unit,
    content: @Composable (PullDetail) -> Unit,
) {
    when (detail) {
        UiState.Loading -> LoadingView(Modifier.fillMaxSize(), label = "Loading pull request…")
        is UiState.Error -> ErrorView(
            detail.error,
            modifier = Modifier.padding(16.dp),
            title = "Couldn't load this pull request",
            onRetry = onRetry,
        )
        is UiState.Loaded -> content(detail.data)
    }
}

@Composable
private fun TabBody(
    tab: PrTab,
    detail: PullDetail,
    state: PrDetailUiState,
    branch: BranchUiState,
    now: Instant,
    actions: PrScreenActions,
) {
    when (tab) {
        PrTab.Conversation -> ConversationTab(
            detail = detail,
            now = now,
            reply = state.reply,
            replyActions = actions.reply,
            onAddLabel = actions.onAddLabel,
            onMerge = actions.onMerge,
            modifier = Modifier.fillMaxSize(),
        )
        PrTab.Checks -> ChecksTab(detail, state.loadedAt, now, Modifier.fillMaxSize())
        PrTab.Branch -> BranchTab(
            detail = detail,
            busy = state.busy,
            local = branch,
            onUpdateBranch = actions.onUpdateBranch,
            localActions = actions.local,
            modifier = Modifier.fillMaxSize(),
        )
        PrTab.Files -> Unit
    }
}
