package io.github.rhizonymph.rostrum.ui.newissue

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.ChevronRow
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RadioVisual
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** `2 labels`, `ada-lin, mkowal`, or "None". */
fun chosenText(chosen: Set<String>): String = if (chosen.isEmpty()) "None" else chosen.sorted().joinToString(", ")

/** The new-issue form, stateless. */
@Composable
fun NewIssueScreen(state: NewIssueUiState, actions: NewIssueActions, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxSize().background(colors.bg).imePadding()) {
        BackTopBar(onBack = onBack) {
            Text("New issue", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.semantics { heading() })
        }
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 12.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            RostrumCard(Modifier.fillMaxWidth()) {
                ChevronRow(onClick = actions::openRepoPicker) {
                    Column {
                        Text("Repository", style = RostrumText.caption, color = colors.textMuted)
                        Text(state.repo ?: "Choose a repository", style = RostrumText.rowTitle, color = if (state.repo == null) colors.accentText else colors.text)
                    }
                }
            }
            RostrumTextField(
                value = state.title,
                onValueChange = actions::onTitleChange,
                placeholder = "Title",
                accessibilityLabel = "Title",
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            )
            SegmentedToggle(
                options = BodyMode.entries.toList(),
                selected = state.mode,
                onSelect = actions::showMode,
                label = { it.name },
                modifier = Modifier.fillMaxWidth(),
            )
            when (state.mode) {
                BodyMode.Write -> RostrumTextField(
                    value = state.body,
                    onValueChange = actions::onBodyChange,
                    placeholder = "Describe the issue (markdown)",
                    accessibilityLabel = "Body",
                    singleLine = false,
                    minHeight = 160.dp,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                )
                BodyMode.Preview -> RostrumCard(Modifier.fillMaxWidth().heightIn(min = 160.dp)) {
                    Column(Modifier.padding(14.dp)) {
                        if (state.preview.isEmpty()) {
                            Text("Nothing to preview.", style = RostrumText.body, color = colors.textSubtle)
                        } else {
                            MarkdownBlocks(state.preview)
                        }
                    }
                }
            }
            RostrumCard(Modifier.fillMaxWidth()) {
                ChoiceRow("Labels", chosenText(state.labels), enabled = state.repo != null) { actions.openPicker(PickerKind.Labels) }
                ChoiceRow("Assignees", chosenText(state.assignees), enabled = state.repo != null) { actions.openPicker(PickerKind.Assignees) }
            }
            (state.submit as? ActionState.Failed)?.let { FieldError("Couldn't open the issue: ${it.error.describe()}") }
        }
        Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(12.dp), horizontalArrangement = Arrangement.End) {
            PrimaryButton("Create issue", actions::submit, enabled = state.canSubmit, busy = state.submit.running, height = 48.dp)
        }
    }
}

@Composable
private fun ChoiceRow(title: String, value: String, enabled: Boolean, onEdit: () -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 14.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = RostrumText.caption, color = colors.textMuted)
            Text(value, style = RostrumText.meta, color = colors.textSecondary, maxLines = 2)
        }
        TextPillButton("Edit", onEdit, enabled = enabled)
    }
}

/** The watched repositories, one to pick. */
@Composable
fun RepoPickerSheet(state: NewIssueUiState, actions: NewIssueActions) {
    val colors = RostrumTheme.colors
    RostrumBottomSheet(onDismiss = actions::closeRepoPicker) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(start = 12.dp, end = 12.dp, bottom = 16.dp)) {
            Text("Repository", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.padding(12.dp).semantics { heading() })
            when (val repos = state.repos) {
                UiState.Loading -> LoadingView(label = "Loading repositories…")
                is UiState.Error -> ErrorView(repos.error, title = "Couldn't load your repositories")
                is UiState.Loaded -> Column(Modifier.verticalScroll(rememberScrollState())) {
                    repos.data.forEach { repo ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = 52.dp)
                                .clip(RoundedCornerShape(12.dp))
                                .selectable(selected = repo == state.repo, role = Role.RadioButton, onClick = { actions.chooseRepo(repo) })
                                .padding(horizontal = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(14.dp),
                        ) {
                            RadioVisual(repo == state.repo)
                            Text(repo, style = RostrumText.rowTitle, color = colors.text)
                        }
                    }
                }
            }
        }
    }
}
