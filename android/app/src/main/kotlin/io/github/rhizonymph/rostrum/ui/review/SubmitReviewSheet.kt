package io.github.rhizonymph.rostrum.ui.review

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RadioVisual
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.format.shortSha
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * "Finish your review": the pending drafts, a summary and the verdict. Its
 * ViewModel lives with the screen that opened it, so a summary typed and
 * dismissed is still there when the sheet is reopened.
 */
@Composable
fun SubmitReviewSheet(
    pr: PrRef,
    onDismiss: () -> Unit,
    onSubmitted: () -> Unit,
) {
    val viewModel = rostrumViewModel(key = "submit-review-$pr") { SubmitReviewViewModel(it.backend, pr) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val composer by viewModel.composer.state.collectAsStateWithLifecycle()
    val submitted by rememberUpdatedState(onSubmitted)
    CollectMessages(viewModel.messages.flow)
    LaunchedEffect(viewModel) { viewModel.load() }
    LaunchedEffect(viewModel) { viewModel.submitted.collect { submitted() } }

    val dismiss = {
        viewModel.composer.dismiss()
        onDismiss()
    }
    RostrumBottomSheet(onDismiss = dismiss) {
        val editing = composer
        if (editing != null) {
            LineCommentContent(editing, ComposerActions.of(viewModel.composer))
        } else {
            SubmitReviewContent(
                state = state,
                actions = SubmitActions(
                    onClose = dismiss,
                    onSummary = viewModel::setSummary,
                    onVerdict = viewModel::setVerdict,
                    onSubmit = viewModel::submit,
                    onDiscard = viewModel::requestDiscard,
                    onEditDraft = viewModel::editDraft,
                    onRetry = viewModel::load,
                ),
            )
        }
    }
    if (state.confirmingDiscard) {
        val count = (state.content as? UiState.Loaded)?.data?.pending?.drafts?.size ?: 0
        ConfirmDialog(
            title = "Discard pending comments?",
            body = "Your ${ReviewLabels.pendingCount(count)} will be deleted. This can't be undone.",
            confirmLabel = "Discard",
            onConfirm = viewModel::confirmDiscard,
            onDismiss = viewModel::cancelDiscard,
            destructive = true,
        )
    }
}

class SubmitActions(
    val onClose: () -> Unit,
    val onSummary: (String) -> Unit,
    val onVerdict: (ReviewEvent) -> Unit,
    val onSubmit: () -> Unit,
    val onDiscard: () -> Unit,
    val onEditDraft: (Long) -> Unit,
    val onRetry: () -> Unit,
)

@Composable
fun SubmitReviewContent(state: SubmitReviewState, actions: SubmitActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
    ) {
        val content = (state.content as? UiState.Loaded)?.data
        Row(verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f).padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Finish your review", style = RostrumText.sheetTitle.copy(lineHeight = RostrumText.screenTitle.lineHeight), color = colors.text, modifier = Modifier.semantics { heading() })
                if (content != null) HeadLine(content)
            }
            RostrumIconButton(RostrumIcons.Close, "Close", actions.onClose, tint = colors.textSecondary)
        }
        when (val loaded = state.content) {
            UiState.Loading -> LoadingView()
            is UiState.Error -> ErrorView(loaded.error, Modifier.padding(top = 16.dp), title = "Couldn't load your review", onRetry = actions.onRetry)
            is UiState.Loaded -> ReviewForm(state, loaded.data, actions)
        }
    }
}

@Composable
private fun HeadLine(content: SubmitContent) {
    val colors = RostrumTheme.colors
    val pending = content.pending
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            "${content.header.repo} #${content.header.number} · head ${shortSha(content.header.headSha)}",
            style = RostrumText.mono12,
            color = colors.textMuted,
        )
        if (pending.drafts.isNotEmpty()) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                StatusDot(if (pending.stale) colors.danger else colors.success)
                Text(
                    if (pending.stale) "head moved since you drafted" else "drafts current",
                    style = RostrumText.mono12,
                    color = if (pending.stale) colors.dangerText else colors.successText,
                )
            }
        }
    }
}

@Composable
private fun ReviewForm(state: SubmitReviewState, content: SubmitContent, actions: SubmitActions) {
    val colors = RostrumTheme.colors
    val drafts = content.pending.drafts
    Text(
        "Pending comments (${drafts.size})",
        style = RostrumText.section,
        color = colors.textMuted,
        modifier = Modifier.padding(top = 16.dp, bottom = 8.dp).semantics { heading() },
    )
    if (drafts.isEmpty()) {
        Text("No pending comments. Tap a line number in a diff to add one.", style = RostrumText.meta, color = colors.textSubtle)
    } else {
        RostrumCard(Modifier.fillMaxWidth()) {
            drafts.forEachIndexed { index, draft ->
                if (index > 0) CardDivider()
                PendingDraftRow(draft, onEdit = { actions.onEditDraft(draft.id) })
            }
        }
    }
    state.staleWarning?.let { StaleWarning(it) }

    Column(Modifier.padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("Summary", style = RostrumText.section, color = colors.textMuted)
        RostrumTextField(
            value = state.summary,
            onValueChange = actions.onSummary,
            placeholder = "Leave a summary (optional)",
            accessibilityLabel = "Summary",
            singleLine = false,
            minHeight = 72.dp,
            fill = colors.bg,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            modifier = Modifier.fillMaxWidth(),
        )
    }

    Column(Modifier.padding(top = 16.dp).selectableGroup(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        ReviewEvent.entries.forEach { event ->
            VerdictRow(
                event = event,
                selected = state.verdict == event,
                blockedReason = state.blockedReason(event),
                subtitle = ReviewRules.verdictSubtitle(event, content.header),
                onSelect = { actions.onVerdict(event) },
            )
        }
    }
    if (!content.header.isYours) {
        val author = content.header.author?.login ?: "the author"
        Text(
            "If $author pushes before you submit, Approve and Request changes are disabled until you re-check the diff.",
            style = RostrumText.caption,
            color = colors.textSubtle,
            modifier = Modifier.padding(top = 10.dp),
        )
    }
    (state.submit as? ActionState.Failed)?.let { FieldError(it.error.describe(), Modifier.padding(top = 10.dp)) }
    (state.discard as? ActionState.Failed)?.let { FieldError(it.error.describe(), Modifier.padding(top = 10.dp)) }

    Row(Modifier.fillMaxWidth().padding(top = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        TextPillButton(
            "Discard drafts",
            actions.onDiscard,
            enabled = drafts.isNotEmpty() && !state.discard.running,
            color = colors.dangerText,
            style = RostrumText.button,
            modifier = Modifier.padding(end = 8.dp),
        )
        Spacer(Modifier.weight(1f))
        PrimaryButton("Submit review", actions.onSubmit, enabled = state.canSubmit, busy = state.submit.running)
    }
}

@Composable
private fun PendingDraftRow(draft: ReviewDraft, onEdit: () -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 14.dp, end = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f).padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(ReviewLabels.location(draft.anchor), style = RostrumText.mono12, color = colors.textSecondary)
            Text(draft.body, style = RostrumText.meta, color = colors.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        RostrumIconButton(RostrumIcons.Edit, ReviewLabels.editDescription(draft.anchor), onEdit, tint = colors.textMuted, iconSize = 18.dp)
    }
}

@Composable
private fun StaleWarning(text: String) {
    val colors = RostrumTheme.colors
    Row(
        Modifier
            .padding(top = 12.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(colors.danger.copy(alpha = 0.12f))
            .padding(12.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.dangerText, modifier = Modifier.size(18.dp))
        Text(text, style = RostrumText.meta, color = colors.textSecondary)
    }
}

@Composable
private fun VerdictRow(event: ReviewEvent, selected: Boolean, blockedReason: String?, subtitle: String, onSelect: () -> Unit) {
    val colors = RostrumTheme.colors
    val enabled = blockedReason == null
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(if (selected) colors.tonal else colors.raised)
            .selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onSelect)
            .padding(horizontal = 14.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(Modifier.size(20.dp)) { RadioVisual(selected && enabled) }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(
                ReviewRules.verdictTitle(event),
                style = RostrumText.rowTitle.copy(fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium),
                color = if (enabled) colors.text else colors.textSubtle,
            )
            Text(blockedReason ?: subtitle, style = RostrumText.caption.copy(lineHeight = RostrumText.chip.lineHeight), color = colors.textMuted)
        }
        if (selected) Icon(RostrumIcons.Check, contentDescription = null, tint = colors.onTonal, modifier = Modifier.size(18.dp))
    }
}

@Preview(widthDp = 412, heightDp = 800)
@Composable
private fun SubmitReviewPreview() {
    val pr = PrRef("RhizoNymph/rostrum", 10)
    val content = SubmitContent(PreviewData.sample { pullHeader(pr) }, PreviewData.sample { pendingReview(pr) })
    val state = SubmitReviewState(content = UiState.Loaded(content), summary = "Clean split. Two nits inline.", verdict = ReviewEvent.Approve)
    RostrumTheme {
        Box(Modifier.background(RostrumTheme.colors.raised)) {
            SubmitReviewContent(state, SubmitActions({}, {}, {}, {}, {}, {}, {}), Modifier.padding(top = 16.dp))
        }
    }
}

@Preview(widthDp = 412, heightDp = 800)
@Composable
private fun SubmitReviewStalePreview() {
    val pr = PrRef("RhizoNymph/rostrum", 9)
    val content = SubmitContent(PreviewData.sample { pullHeader(pr) }, PreviewData.sample { pendingReview(pr) })
    RostrumTheme {
        Box(Modifier.background(RostrumTheme.colors.raised)) {
            SubmitReviewContent(SubmitReviewState(content = UiState.Loaded(content)), SubmitActions({}, {}, {}, {}, {}, {}, {}))
        }
    }
}
