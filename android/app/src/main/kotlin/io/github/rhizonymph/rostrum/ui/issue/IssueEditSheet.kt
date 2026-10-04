package io.github.rhizonymph.rostrum.ui.issue

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The editor's heading. */
fun editorTitle(field: EditField): String = if (field == EditField.Title) "Edit title" else "Edit description"

/**
 * The title or the description in a sheet (both are saved together): the
 * description with Write/Preview. A conflict opens [EditConflictDialog].
 */
@Composable
fun IssueEditSheet(editor: IssueEditor, actions: IssueActions) {
    RostrumBottomSheet(onDismiss = actions::closeEditor) {
        IssueEditContent(editor, actions)
    }
    editor.conflict?.let { EditConflictDialog(it, actions) }
}

@Composable
fun IssueEditContent(editor: IssueEditor, actions: IssueActions) {
    val colors = RostrumTheme.colors
    Column(
        Modifier.fillMaxWidth().navigationBarsPadding().imePadding().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(editorTitle(editor.field), style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.semantics { heading() })
        when (editor.field) {
            EditField.Title -> RostrumTextField(
                value = editor.title,
                onValueChange = actions::onEditTitle,
                placeholder = "Title",
                accessibilityLabel = "Title",
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            )
            EditField.Description -> {
                SegmentedToggle(
                    options = EditMode.entries.toList(),
                    selected = editor.mode,
                    onSelect = actions::showEditMode,
                    label = { it.name },
                    modifier = Modifier.fillMaxWidth(),
                )
                when (editor.mode) {
                    EditMode.Write -> RostrumTextField(
                        value = editor.body,
                        onValueChange = actions::onEditBody,
                        placeholder = "Description (markdown)",
                        accessibilityLabel = "Description",
                        singleLine = false,
                        minHeight = 180.dp,
                    )
                    EditMode.Preview -> RostrumCard(Modifier.fillMaxWidth().heightIn(min = 180.dp)) {
                        Column(Modifier.padding(14.dp)) {
                            if (editor.preview.isEmpty()) {
                                Text("Nothing to preview.", style = RostrumText.body, color = colors.textSubtle)
                            } else {
                                MarkdownBlocks(editor.preview)
                            }
                        }
                    }
                }
            }
        }
        (editor.save as? ActionState.Failed)?.let { FieldError("Couldn't save: ${it.error.describe()}") }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, androidx.compose.ui.Alignment.End)) {
            TextPillButton("Cancel", actions::closeEditor, enabled = !editor.save.running)
            PrimaryButton("Save", actions::saveEdit, enabled = editor.canSave, busy = editor.save.running)
        }
    }
}

/** GitHub's version changed meanwhile: take it, or send yours over it. */
@Composable
fun EditConflictDialog(conflict: EditConflictInfo, actions: IssueActions) {
    val colors = RostrumTheme.colors
    AlertDialog(
        onDismissRequest = {},
        containerColor = colors.raised,
        title = { Text("Changed on GitHub", style = RostrumText.sheetTitle, color = colors.text) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "Someone changed this issue's title or description while you edited it. Reload to keep theirs, " +
                        "or overwrite it with yours.",
                    style = RostrumText.body,
                    color = colors.textSecondary,
                )
                Text("Their title: ${conflict.title}", style = RostrumText.meta, color = colors.text)
            }
        },
        confirmButton = { TextPillButton("Overwrite", actions::overwrite, color = colors.dangerText, style = RostrumText.button) },
        dismissButton = { TextPillButton("Reload", actions::reloadTheirs) },
    )
}
