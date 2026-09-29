package io.github.rhizonymph.rostrum.ui.pr

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.LocalAppContainer
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.navigation.PrTab
import io.github.rhizonymph.rostrum.ui.pr.branch.BranchViewModel
import io.github.rhizonymph.rostrum.ui.pr.branch.LocalActions
import io.github.rhizonymph.rostrum.ui.pr.conversation.ReplyActions
import io.github.rhizonymph.rostrum.ui.pr.labels.LabelPickerSheet
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeActions
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeSheet
import kotlinx.coroutines.delay

/** Close and reopen are confirmed; this is which one is being asked about. */
private enum class PendingConfirm { Close, Reopen }

/**
 * The pull request screen: four tabs over one pull request. The Files tab
 * and the review sheet come from the navigation graph ([filesTab],
 * [reviewSheet]) so this package never depends on them.
 */
@Composable
fun PullRequestRoute(
    pr: PrRef,
    initialTab: PrTab,
    onBack: () -> Unit,
    filesTab: @Composable (Modifier) -> Unit,
    reviewSheet: @Composable (onDismiss: () -> Unit, onSubmitted: () -> Unit) -> Unit,
    modifier: Modifier = Modifier,
) {
    val clock = LocalAppContainer.current.clock
    val detailVm = rostrumViewModel(key = "pr-detail:$pr") { PrDetailViewModel(pr, it.backend, it.clock) }
    val branchVm = rostrumViewModel(key = "pr-branch:$pr") { BranchViewModel(pr, it.backend) }
    val state by detailVm.state.collectAsStateWithLifecycle()
    val branch by branchVm.state.collectAsStateWithLifecycle()
    CollectMessages(detailVm.messages.flow)
    CollectMessages(branchVm.messages.flow)

    var tab by rememberSaveable { mutableStateOf(initialTab) }
    var showReview by rememberSaveable { mutableStateOf(false) }
    var confirm by rememberSaveable { mutableStateOf<PendingConfirm?>(null) }
    val now by produceState(clock.instant(), clock) {
        while (true) {
            delay(30_000)
            value = clock.instant()
        }
    }

    LaunchedEffect(tab) { if (tab == PrTab.Branch) branchVm.ensureLoaded() }
    // Coming back from a diff (new drafts) or another app: reload, but not on the first resume.
    var resumedOnce by remember { mutableStateOf(false) }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        if (resumedOnce) detailVm.refresh() else resumedOnce = true
    }

    PrScreen(
        state = state,
        branch = branch,
        tab = tab,
        now = now,
        filesTab = filesTab,
        modifier = modifier,
        actions = PrScreenActions(
            onBack = onBack,
            onTab = { tab = it },
            onRetry = detailVm::refresh,
            menu = PrMenuActions(
                onToggleDraft = detailVm::toggleDraft,
                onRequestClose = { confirm = PendingConfirm.Close },
                onRequestReopen = { confirm = PendingConfirm.Reopen },
                onRefresh = {
                    detailVm.refresh()
                    if (tab == PrTab.Branch) branchVm.refresh()
                },
            ),
            onCommentText = detailVm::setComment,
            onSendComment = detailVm::postComment,
            onReview = { showReview = true },
            reply = ReplyActions(detailVm::startReply, detailVm::setReplyText, detailVm::sendReply, detailVm::cancelReply),
            onAddLabel = detailVm::openLabels,
            onMerge = detailVm::openMerge,
            onUpdateBranch = detailVm::updateBranch,
            local = LocalActions(branchVm::runOp, branchVm::abort, branchVm::setAutostash, branchVm::refresh),
        ),
    )

    if (showReview) {
        reviewSheet(
            { showReview = false },
            {
                showReview = false
                detailVm.refresh()
            },
        )
    }

    state.labels?.let { picker ->
        LabelPickerSheet(
            state = picker,
            applied = state.detail.dataOrNull()?.header?.labels?.mapTo(mutableSetOf()) { it.name }.orEmpty(),
            onToggle = detailVm::toggleLabel,
            onRetry = detailVm::openLabels,
            onDismiss = detailVm::closeLabels,
        )
    }

    val detail = state.detail.dataOrNull()
    val form = state.merge
    if (detail != null && form != null) {
        MergeSheet(
            detail = detail,
            form = form,
            actions = MergeActions(
                onMethod = detailVm::setMergeMethod,
                onTitle = detailVm::setMergeTitle,
                onMessage = detailVm::setMergeMessage,
                onConfirm = detailVm::confirmMerge,
                onDismiss = detailVm::dismissMerge,
            ),
        )
    }

    when (confirm) {
        PendingConfirm.Close -> ConfirmDialog(
            title = "Close #${pr.number}?",
            body = "The pull request is closed without merging. You can reopen it later.",
            confirmLabel = "Close pull request",
            destructive = true,
            onConfirm = {
                confirm = null
                detailVm.close()
            },
            onDismiss = { confirm = null },
        )
        PendingConfirm.Reopen -> ConfirmDialog(
            title = "Reopen #${pr.number}?",
            body = "The pull request opens again for review, as it was when it was closed.",
            confirmLabel = "Reopen",
            onConfirm = {
                confirm = null
                detailVm.reopen()
            },
            onDismiss = { confirm = null },
        )
        null -> Unit
    }
}
