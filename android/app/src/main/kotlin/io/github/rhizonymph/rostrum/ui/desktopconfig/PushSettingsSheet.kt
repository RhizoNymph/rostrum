package io.github.rhizonymph.rostrum.ui.desktopconfig

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.ConfigChange
import io.github.rhizonymph.rostrum.data.model.ConfigField
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.format.MINUS
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** What sending this profile's settings would change on the desktop, and Send. Hidden while Closed. */
@Composable
fun PushSettingsSheet(state: PushSheetState, actions: PushSheetActions, machine: String?) {
    if (state == PushSheetState.Closed) return
    RostrumBottomSheet(onDismiss = actions::dismiss) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 16.dp)
                .navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            val name = (state as? PushSheetState.Ready)?.preview?.machine ?: machine ?: "your desktop"
            Text(
                PushConfigText.sheetTitle(name),
                style = RostrumText.sheetTitle,
                color = RostrumTheme.colors.text,
                modifier = Modifier.padding(top = 8.dp).semantics { heading() },
            )
            when (state) {
                PushSheetState.Closed -> Unit
                PushSheetState.Loading -> LoadingView(label = "Reading $name's settings")
                is PushSheetState.Failed -> ErrorView(state.error, title = "Couldn't read $name's settings", onRetry = actions::retry)
                is PushSheetState.Ready -> PushReady(state, actions)
            }
        }
    }
}

@Composable
private fun PushReady(state: PushSheetState.Ready, actions: PushSheetActions) {
    val colors = RostrumTheme.colors
    val machine = state.preview.machine
    if (state.stale) StaleBanner(machine)
    if (state.nothingToSend) {
        Text(PushConfigText.nothingToSend(machine), style = RostrumText.body, color = colors.textSecondary)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            TextPillButton("Close", actions::dismiss)
        }
        return
    }
    if (!state.stale) Text(PushConfigText.body(machine), style = RostrumText.body, color = colors.textSecondary)
    ConfigChangeList(state.changes)
    (state.send as? ActionState.Failed)?.let { FieldError("Couldn't send: ${it.error.describe()}") }
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TextPillButton("Cancel", actions::dismiss, enabled = !state.send.running)
        PrimaryButton(if (state.stale) "Send again" else "Send", actions::send, busy = state.send.running)
    }
}

@Composable
private fun StaleBanner(machine: String) {
    val colors = RostrumTheme.colors
    Column(
        Modifier
            .fillMaxWidth()
            .background(colors.warning.copy(alpha = 0.15f), RoundedCornerShape(12.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(PushConfigText.CHANGED_SINCE, style = RostrumText.label, color = colors.warningText)
        Text(PushConfigText.changedSince(machine), style = RostrumText.caption, color = colors.textSecondary)
    }
}

/** Each change on its own row: a list's additions and removals, or a value's before → after. */
@Composable
fun ConfigChangeList(changes: List<ConfigChangeView>, modifier: Modifier = Modifier) {
    RostrumCard(modifier.fillMaxWidth()) {
        changes.forEachIndexed { index, change ->
            if (index > 0) CardDivider()
            ChangeRow(change)
        }
    }
}

@Composable
private fun ChangeRow(change: ConfigChangeView) {
    val colors = RostrumTheme.colors
    Column(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(change.label, style = RostrumText.label, color = colors.text)
        when (change) {
            is ConfigChangeView.Value -> Text(
                "${change.before} → ${change.after}",
                style = RostrumText.mono12,
                color = colors.textSecondary,
                modifier = Modifier.clearAndSetSemantics { contentDescription = "from ${change.before} to ${change.after}" },
            )
            is ConfigChangeView.ListDiff -> {
                if (change.reordered) Text("Reordered", style = RostrumText.caption, color = colors.textMuted)
                change.added.forEach { ListItem("+", it, colors.successText, "adds $it") }
                change.removed.forEach { ListItem(MINUS, it, colors.dangerText, "removes $it") }
            }
        }
    }
}

@Composable
private fun ListItem(marker: String, text: String, color: androidx.compose.ui.graphics.Color, description: String) {
    Row(Modifier.clearAndSetSemantics { contentDescription = description }) {
        Text(marker, style = RostrumText.monoStrong13, color = color, modifier = Modifier.width(18.dp))
        Text(text, style = RostrumText.mono12, color = RostrumTheme.colors.textSecondary)
    }
}

@Preview(widthDp = 412)
@Composable
private fun PushReadyPreview() {
    val preview = DesktopConfigPreview(
        machine = "framework", repos = listOf("a/one"), added = emptyList(), removed = emptyList(),
        prsPerRepo = 25, hideDrafts = true, hideEmptyRepos = true, authors = emptyList(), includeInvolved = false,
        autostash = true, changesAnything = true, revision = "r3", issuesPerRepo = 25,
        pushChanges = listOf(
            ConfigChange(ConfigField.Repos, "Repositories", "a/one, b/two", "a/one, c/three"),
            ConfigChange(ConfigField.PrsPerRepo, "Pull requests per repository", "25", "30"),
            ConfigChange(ConfigField.HideDrafts, "Hide drafts", "true", "false"),
        ),
    )
    RostrumTheme {
        Column(Modifier.background(RostrumTheme.colors.raised).padding(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            PushReady(PushSheetState.Ready(preview, stale = true), NoPushSheetActions)
        }
    }
}
