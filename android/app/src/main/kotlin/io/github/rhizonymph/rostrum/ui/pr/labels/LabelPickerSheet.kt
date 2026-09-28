package io.github.rhizonymph.rostrum.ui.pr.labels

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
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.CheckboxVisual
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.labelColors
import io.github.rhizonymph.rostrum.ui.pr.LabelPickerState
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** Every label the repository defines, checked where the pull request has it. */
@Composable
fun LabelPickerSheet(
    state: LabelPickerState,
    applied: Set<String>,
    onToggle: (String) -> Unit,
    onRetry: () -> Unit,
    onDismiss: () -> Unit,
) {
    RostrumBottomSheet(onDismiss = onDismiss) {
        LabelPickerContent(state, applied, onToggle, onRetry, onDismiss)
    }
}

@Composable
fun LabelPickerContent(
    state: LabelPickerState,
    applied: Set<String>,
    onToggle: (String) -> Unit,
    onRetry: () -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth().navigationBarsPadding().padding(start = 12.dp, end = 12.dp, bottom = 16.dp)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Labels", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.weight(1f))
            if (applied.isNotEmpty()) Text("${applied.size} applied", style = RostrumText.mono12, color = colors.accentText)
        }
        when (val available = state.available) {
            UiState.Loading -> LoadingView(label = "Loading labels…")
            is UiState.Error -> ErrorView(available.error, title = "Couldn't load labels", onRetry = onRetry)
            is UiState.Loaded -> if (available.data.isEmpty()) {
                EmptyView("No labels defined", body = "Create labels on GitHub to use them here.")
            } else {
                Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                    available.data.forEach { label ->
                        LabelRow(label, checked = label.name in applied, pending = state.pending, onToggle = onToggle)
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
private fun LabelRow(label: LabelView, checked: Boolean, pending: String?, onToggle: (String) -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clip(RoundedCornerShape(12.dp))
            .toggleable(value = checked, enabled = pending == null, role = Role.Checkbox, onValueChange = { onToggle(label.name) })
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(Modifier.size(14.dp).clip(CircleShape).background(labelColors(label.argb, colors).solid))
        Text(label.name, style = RostrumText.rowTitle, color = colors.text, modifier = Modifier.weight(1f))
        if (pending == label.name) {
            CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
        } else {
            CheckboxVisual(checked)
        }
    }
}
