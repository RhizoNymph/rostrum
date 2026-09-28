package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.LocalAppContainer
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.review.ComposerActions
import io.github.rhizonymph.rostrum.ui.review.LineCommentSheet
import io.github.rhizonymph.rostrum.ui.review.SubmitReviewSheet

/**
 * One file's diff, opened from the Files tab. Previous/next move through the
 * ranked files inside this screen; Back returns to the overview.
 */
@Composable
fun FileDiffRoute(
    pr: PrRef,
    fileIndex: Int,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val clock = LocalAppContainer.current.clock
    val viewModel = rostrumViewModel(key = "file-diff-$pr-$fileIndex") { FileDiffViewModel(it.backend, pr, fileIndex) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val composer by viewModel.composer.state.collectAsStateWithLifecycle()
    var finishing by rememberSaveable { mutableStateOf(false) }
    CollectMessages(viewModel.messages.flow)

    val now = remember(state.diff) { clock.instant() }
    val actions = remember(viewModel, onBack) {
        DiffScreenActions(
            onBack = onBack,
            onPrevious = viewModel::previousFile,
            onNext = viewModel::nextFile,
            onToggleSoftWrap = viewModel::toggleSoftWrap,
            onToggleViewed = viewModel::toggleViewed,
            onRetry = viewModel::retry,
            onFinishReview = { finishing = true },
            body = DiffBodyActions(
                onLineTap = viewModel::onLineNumberTap,
                onSelectionStart = viewModel::onSelectionStart,
                onSelectionMove = viewModel::onSelectionMove,
                onSelectionEnd = viewModel::onSelectionEnd,
                onSelectionCancel = viewModel::clearSelection,
                onEditDraft = viewModel::editDraft,
                onDeleteDraft = viewModel::deleteDraft,
                onStartReply = viewModel::startReply,
                onReplyText = viewModel::setReplyText,
                onSendReply = viewModel::sendReply,
                onCancelReply = viewModel::cancelReply,
            ),
        )
    }
    DiffScreen(state, now, actions, modifier)

    composer?.let { LineCommentSheet(it, remember(viewModel) { ComposerActions.of(viewModel.composer) }) }
    if (finishing) {
        SubmitReviewSheet(
            pr = pr,
            onDismiss = {
                finishing = false
                viewModel.refreshPending()
            },
            onSubmitted = {
                finishing = false
                viewModel.onReviewSubmitted()
            },
        )
    }
}
