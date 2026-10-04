package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.UserRef
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** One choice in a [PickerSheet]: a label (with its colour) or a person (with an avatar). */
sealed interface PickerOption {
    /** What the selection set holds: the label's name or the login. */
    val key: String

    data class Label(val label: LabelView) : PickerOption {
        override val key: String get() = label.name
    }

    data class Person(val user: UserRef) : PickerOption {
        override val key: String get() = user.login
    }
}

/** Which list a picker edits. */
enum class PickerKind(val title: String, val emptyTitle: String, val emptyBody: String, val noun: String) {
    Labels("Labels", "No labels defined", "Create labels on GitHub to use them here.", "labels"),
    Assignees("Assignees", "Nobody to assign", "Only collaborators can be assigned.", "people"),
}

/** An open picker: its options, and the one whose change is in flight. */
data class PickerState(val kind: PickerKind, val options: UiState<List<PickerOption>>, val pending: String? = null)

/**
 * A multi-select sheet: every option, checked where [selected] has it.
 * Toggling one either applies it at once (the issue screen, which shows
 * [PickerState.pending] meanwhile) or edits a draft (the new-issue form).
 */
@Composable
fun PickerSheet(
    state: PickerState,
    selected: Set<String>,
    onToggle: (String) -> Unit,
    onRetry: () -> Unit,
    onDismiss: () -> Unit,
) {
    RostrumBottomSheet(onDismiss = onDismiss) {
        PickerContent(state, selected, onToggle, onRetry, onDismiss)
    }
}

@Composable
fun PickerContent(
    state: PickerState,
    selected: Set<String>,
    onToggle: (String) -> Unit,
    onRetry: () -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth().navigationBarsPadding().padding(start = 12.dp, end = 12.dp, bottom = 16.dp)) {
        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(state.kind.title, style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.weight(1f).semantics { heading() })
            if (selected.isNotEmpty()) Text("${selected.size} chosen", style = RostrumText.mono12, color = colors.accentText)
        }
        when (val options = state.options) {
            UiState.Loading -> LoadingView(label = "Loading ${state.kind.noun}…")
            is UiState.Error -> ErrorView(options.error, title = "Couldn't load ${state.kind.noun}", onRetry = onRetry)
            is UiState.Loaded -> if (options.data.isEmpty()) {
                EmptyView(state.kind.emptyTitle, body = state.kind.emptyBody)
            } else {
                Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                    options.data.forEach { option ->
                        OptionRow(option, checked = option.key in selected, pending = state.pending, onToggle = onToggle)
                    }
                }
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 12.dp, end = 4.dp), horizontalArrangement = Arrangement.End) {
            PrimaryButton("Done", onDone)
        }
    }
}

@Composable
private fun OptionRow(option: PickerOption, checked: Boolean, pending: String?, onToggle: (String) -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clip(RoundedCornerShape(12.dp))
            .toggleable(value = checked, enabled = pending == null, role = Role.Checkbox, onValueChange = { onToggle(option.key) })
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        when (option) {
            is PickerOption.Label -> Box(Modifier.size(14.dp).clip(CircleShape).background(labelColors(option.label.argb, colors).solid))
            is PickerOption.Person -> Avatar(option.user.login, size = 28.dp)
        }
        Text(option.key, style = RostrumText.rowTitle, color = colors.text, modifier = Modifier.weight(1f))
        if (pending == option.key) {
            CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
        } else {
            CheckboxVisual(checked)
        }
    }
}
