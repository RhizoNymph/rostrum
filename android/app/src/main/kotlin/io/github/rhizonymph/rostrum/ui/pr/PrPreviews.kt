package io.github.rhizonymph.rostrum.ui.pr

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.navigation.PrTab
import io.github.rhizonymph.rostrum.ui.pr.branch.BranchUiState
import io.github.rhizonymph.rostrum.ui.pr.branch.LocalActions
import io.github.rhizonymph.rostrum.ui.pr.branch.LocalCardState
import io.github.rhizonymph.rostrum.ui.pr.conversation.ReplyActions
import io.github.rhizonymph.rostrum.ui.pr.labels.LabelPickerContent
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeActions
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeFormState
import io.github.rhizonymph.rostrum.ui.pr.merge.MergeSheetContent
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

private val SamplePr = PrRef("RhizoNymph/rostrum", 10)

private fun previewActions() = PrScreenActions(
    onBack = {},
    onTab = {},
    onRetry = {},
    menu = PrMenuActions({}, {}, {}, {}),
    onCommentText = {},
    onSendComment = {},
    onReview = {},
    reply = ReplyActions({}, {}, {}, {}),
    onAddLabel = {},
    onMerge = {},
    onUpdateBranch = {},
    local = LocalActions({}, {}, {}, {}),
)

private fun loadedState(): PrDetailUiState {
    val detail = PreviewData.sample { pullDetail(SamplePr) }
    return PrDetailUiState(SamplePr, UiState.Loaded(detail), loadedAt = PreviewData.clock.instant())
}

@Composable
private fun ScreenPreview(state: PrDetailUiState, tab: PrTab, branch: BranchUiState = BranchUiState()) {
    RostrumTheme {
        PrScreen(
            state = state,
            branch = branch,
            tab = tab,
            now = PreviewData.clock.instant(),
            actions = previewActions(),
            filesTab = { Box(it) },
        )
    }
}

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun ConversationPreview() = ScreenPreview(loadedState(), PrTab.Conversation)

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun ChecksPreview() = ScreenPreview(loadedState(), PrTab.Checks)

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun BranchPreview() {
    val local = PreviewData.sample { localStatus(SamplePr) }
    ScreenPreview(loadedState(), PrTab.Branch, BranchUiState(LocalCardState.Ready("nymph-desk", local)))
}

@Preview(widthDp = 412, heightDp = 400)
@Composable
private fun LoadingPreview() = ScreenPreview(PrDetailUiState(SamplePr), PrTab.Conversation)

@Preview(widthDp = 412, heightDp = 400)
@Composable
private fun ErrorPreview() =
    ScreenPreview(PrDetailUiState(SamplePr, UiState.Error(BackendError.Network("connection reset"))), PrTab.Conversation)

@Preview(widthDp = 412, heightDp = 700)
@Composable
private fun MergeSheetPreview() {
    val detail = PreviewData.sample {
        submitReview(SamplePr, ReviewEvent.Approve, "", includeDrafts = false)
        pullDetail(SamplePr)
    }
    RostrumTheme {
        Box(Modifier.background(RostrumTheme.colors.raised)) {
            MergeSheetContent(
                detail = detail,
                form = MergeFormState.forMethod(MergeMethod.Merge, detail.header),
                actions = MergeActions({}, {}, {}, {}, {}),
            )
        }
    }
}

@Preview(widthDp = 412, heightDp = 600)
@Composable
private fun LabelPickerPreview() {
    val labels = PreviewData.sample { repositoryLabels(SamplePr.repo) }
    RostrumTheme {
        Box(Modifier.background(RostrumTheme.colors.raised)) {
            LabelPickerContent(
                state = LabelPickerState(UiState.Loaded(labels)),
                applied = setOf("diff_review"),
                onToggle = {},
                onRetry = {},
                onDone = {},
            )
        }
    }
}
