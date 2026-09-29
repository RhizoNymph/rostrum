package io.github.rhizonymph.rostrum.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.desktopconfig.DesktopConfigPreviewView
import io.github.rhizonymph.rostrum.ui.desktopconfig.DesktopConfigText
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The preview of the desktop's settings, and "Replace settings". Hidden while [state] is Closed. */
@Composable
fun CopySettingsSheet(state: CopySheetState, actions: CopySheetActions, machine: String?) {
    if (state == CopySheetState.Closed) return
    val colors = RostrumTheme.colors
    RostrumBottomSheet(onDismiss = actions::dismiss) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 16.dp)
                .navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            val name = (state as? CopySheetState.Ready)?.preview?.machine ?: machine ?: "your desktop"
            Text(
                DesktopConfigText.sheetTitle(name),
                style = RostrumText.sheetTitle,
                color = colors.text,
                modifier = Modifier.padding(top = 8.dp).semantics { heading() },
            )
            when (state) {
                CopySheetState.Closed -> Unit
                CopySheetState.Loading -> LoadingView(label = "Reading $name's settings")
                is CopySheetState.Failed -> ErrorView(
                    state.error,
                    title = "Couldn't read $name's settings",
                    onRetry = actions::retry,
                )
                is CopySheetState.Ready -> ReadyContent(state, actions)
            }
        }
    }
}

@Composable
private fun ReadyContent(state: CopySheetState.Ready, actions: CopySheetActions) {
    val colors = RostrumTheme.colors
    val preview = state.preview
    if (!preview.changesAnything) {
        Text(DesktopConfigText.unchanged(preview.machine), style = RostrumText.body, color = colors.textSecondary)
        DesktopConfigPreviewView(preview)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            TextPillButton("Close", actions::dismiss)
        }
        return
    }
    Text(DesktopConfigText.replaceBody(preview.machine), style = RostrumText.body, color = colors.textSecondary)
    state.removalWarning?.let { Text(it, style = RostrumText.label, color = colors.dangerText) }
    DesktopConfigPreviewView(preview, changes = state.changes)
    (state.copy as? ActionState.Failed)?.let { FieldError("Couldn't copy: ${it.error.describe()}") }
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TextPillButton("Cancel", actions::dismiss, enabled = !state.copy.running)
        PrimaryButton("Replace settings", actions::replace, busy = state.copy.running)
    }
}
