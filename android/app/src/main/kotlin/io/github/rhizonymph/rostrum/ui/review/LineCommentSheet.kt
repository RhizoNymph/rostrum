package io.github.rhizonymph.rostrum.ui.review

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** What the line comment editor can ask of its owner. */
class ComposerActions(
    val onText: (String) -> Unit,
    val onTab: (EditorTab) -> Unit,
    val onAddToReview: () -> Unit,
    val onCommentNow: () -> Unit,
    val onSave: () -> Unit,
    val onDelete: () -> Unit,
    val onDismiss: () -> Unit,
) {
    companion object {
        fun of(composer: CommentComposer) = ComposerActions(
            onText = composer::setText,
            onTab = composer::setTab,
            onAddToReview = composer::addToReview,
            onCommentNow = composer::commentNow,
            onSave = composer::save,
            onDelete = composer::delete,
            onDismiss = composer::dismiss,
        )
    }
}

/** The line comment editor in its own bottom sheet (from the diff). */
@Composable
fun LineCommentSheet(state: ComposerState, actions: ComposerActions) {
    RostrumBottomSheet(onDismiss = actions.onDismiss) {
        LineCommentContent(state, actions)
    }
}

/**
 * The editor itself, also shown inside the submit sheet when a pending
 * comment is edited from there: anchor chip, Write/Preview, the text, where
 * it will attach, and the actions for a new comment or an edit.
 */
@Composable
fun LineCommentContent(state: ComposerState, actions: ComposerActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val focus = remember { FocusRequester() }
    val editing = state.mode is ComposerMode.Edit
    Column(
        modifier = modifier
            .fillMaxWidth()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            AnchorChip(state.chip)
            Spacer(Modifier.weight(1f))
            RostrumIconButton(
                RostrumIcons.Close,
                contentDescription = if (editing) "Close without saving" else "Discard comment",
                onClick = actions.onDismiss,
                tint = colors.textSecondary,
            )
        }
        SegmentedToggle(
            options = EditorTab.entries,
            selected = state.tab,
            onSelect = actions.onTab,
            label = { it.name },
            height = 32.dp,
            modifier = Modifier.width(184.dp),
        )
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Comment", style = RostrumText.section, color = colors.textMuted)
            when (state.tab) {
                EditorTab.Write -> RostrumTextField(
                    value = state.text,
                    onValueChange = actions.onText,
                    placeholder = "Leave a comment",
                    accessibilityLabel = "Comment",
                    singleLine = false,
                    minHeight = 120.dp,
                    fill = colors.bg,
                    enabled = !state.action.running,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                )
                EditorTab.Preview -> PreviewBox(state)
            }
        }
        Text(state.note, style = RostrumText.caption, color = colors.textSubtle)
        state.blockedReason?.let { FieldError(it) }
        (state.action as? ActionState.Failed)?.let { FieldError(it.error.describe()) }
        state.commentNowNote?.let { Text(it, style = RostrumText.caption, color = colors.textSubtle) }
        if (editing) EditActions(state, actions) else NewActions(state, actions)
    }
    LaunchedEffect(state.mode) {
        if (state.tab == EditorTab.Write) focus.requestFocus()
    }
}

@Composable
private fun AnchorChip(text: String) {
    val colors = RostrumTheme.colors
    Row(
        modifier = Modifier
            .height(26.dp)
            .clip(RoundedCornerShape(6.dp))
            .background(colors.accent.copy(alpha = 0.15f))
            .padding(horizontal = 10.dp)
            .semantics(mergeDescendants = true) { contentDescription = "Commenting on $text" },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(RostrumIcons.File, contentDescription = null, tint = colors.accentText, modifier = Modifier.size(14.dp))
        Text(text, style = RostrumText.mono12.copy(fontWeight = FontWeight.Medium), color = colors.accentText, maxLines = 1)
    }
}

@Composable
private fun PreviewBox(state: ComposerState) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(12.dp)
    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 120.dp)
            .clip(shape)
            .background(colors.bg)
            .border(1.dp, colors.borderStrong, shape)
            .padding(12.dp),
    ) {
        if (state.preview.isEmpty()) {
            Text("Nothing to preview", style = RostrumText.body, color = colors.textSubtle)
        } else {
            MarkdownBlocks(state.preview)
        }
    }
}

@Composable
private fun NewActions(state: ComposerState, actions: ComposerActions) {
    val colors = RostrumTheme.colors
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TextPillButton("Comment now", actions.onCommentNow, enabled = state.canSend, style = RostrumText.button)
        PrimaryButton(
            text = "Add to review",
            onClick = actions.onAddToReview,
            enabled = state.canSend,
            busy = state.action.running,
            trailing = {
                Box(
                    Modifier
                        .padding(start = 10.dp)
                        .sizeIn(minWidth = 22.dp, minHeight = 22.dp)
                        .clip(RoundedCornerShape(11.dp))
                        .background(colors.onAccent)
                        .padding(horizontal = 6.dp)
                        .semantics { contentDescription = "${state.badge} pending" },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(state.badge.toString(), style = RostrumText.mono11.copy(fontWeight = FontWeight.SemiBold), color = colors.accentText)
                }
            },
        )
    }
}

@Composable
private fun EditActions(state: ComposerState, actions: ComposerActions) {
    val colors = RostrumTheme.colors
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        TextPillButton("Delete", actions.onDelete, enabled = !state.action.running, color = colors.dangerText, style = RostrumText.button)
        Spacer(Modifier.weight(1f))
        PrimaryButton("Save", actions.onSave, enabled = state.canSend, busy = state.action.running)
    }
}

@Preview(widthDp = 412, heightDp = 460)
@Composable
private fun LineCommentPreview() {
    val path = "crates/rostrum-diff/src/overview.rs"
    val text = "Could `churn` and `weight` be methods on `DiffFile`? Both only read `additions`/`deletions`."
    val state = ComposerState(
        mode = ComposerMode.New(CommentTarget(CommentAnchor(path, 56, Side.Right), CommentAnchor(path, 53, Side.Right))),
        text = text,
        preview = PreviewData.backend().renderMarkdown(text),
        otherPending = 2,
        stale = false,
    )
    RostrumTheme {
        Box(Modifier.background(RostrumTheme.colors.raised)) {
            LineCommentContent(state, ComposerActions({}, {}, {}, {}, {}, {}, {}), Modifier.padding(top = 16.dp))
        }
    }
}
