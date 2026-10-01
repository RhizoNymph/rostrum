package io.github.rhizonymph.rostrum.ui.profiles

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * Settings' "Profiles" section: every profile (the active one checked; tap
 * another to switch), each with Rename and Remove, and ways to add one.
 */
@Composable
fun ProfilesSettingsSection(
    onPairDesktop: () -> Unit,
    onAddTokenProfile: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel = rostrumViewModel { container -> ProfilesSettingsViewModel(container.profiles, container.appMessages) }
    val switcher = rostrumViewModel { container -> ProfileSwitcherViewModel(container.profiles, container.appMessages) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val switching by switcher.state.collectAsStateWithLifecycle()
    ProfilesSectionContent(
        state = state,
        switchingTo = switching.switching,
        actions = viewModel,
        onPick = { switcher.switchTo(it) },
        onPairDesktop = onPairDesktop,
        onAddTokenProfile = onAddTokenProfile,
        modifier = modifier,
    )
}

@Composable
fun ProfilesSectionContent(
    state: ProfilesSectionState,
    switchingTo: ProfileId?,
    actions: ProfilesSectionActions,
    onPick: (ProfileId) -> Unit,
    onPairDesktop: () -> Unit,
    onAddTokenProfile: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader("Profiles", trailing = state.rows.size.toString())
        RostrumCard(Modifier.fillMaxWidth()) {
            state.rows.forEachIndexed { index, row ->
                if (index > 0) CardDivider()
                ProfileRowView(
                    row = row,
                    switching = switchingTo == row.id,
                    onClick = { onPick(row.id) },
                    trailing = {
                        ProfileMenu(
                            label = row.label,
                            onRename = { actions.startRename(row.id) },
                            onRemove = { actions.askRemove(row.id) },
                        )
                    },
                )
            }
            CardDivider()
            Column(Modifier.padding(horizontal = 4.dp, vertical = 2.dp)) {
                TextPillButton("Pair another desktop", onPairDesktop)
                TextPillButton("Add a GitHub token profile", onAddTokenProfile)
            }
        }
    }
    state.rename?.let { RenameProfileDialog(it, actions) }
    state.remove?.let { dialog ->
        ConfirmDialog(
            title = dialog.title,
            body = dialog.body + ((dialog.action as? ActionState.Failed)?.let { "\n\nCouldn't remove it: ${it.error.describe()}" } ?: ""),
            confirmLabel = if (dialog.action.running) "Removing…" else "Remove",
            destructive = true,
            onConfirm = actions::confirmRemove,
            onDismiss = actions::dismissDialog,
        )
    }
}

@Composable
private fun ProfileMenu(label: String, onRename: () -> Unit, onRemove: () -> Unit) {
    val colors = RostrumTheme.colors
    var open by remember { mutableStateOf(false) }
    Box {
        RostrumIconButton(RostrumIcons.MoreVert, "Options for $label", onClick = { open = true }, iconSize = 20.dp)
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = colors.raised) {
            DropdownMenuItem(
                text = { Text("Rename", style = RostrumText.label, color = colors.text) },
                onClick = { open = false; onRename() },
            )
            DropdownMenuItem(
                text = { Text("Remove", style = RostrumText.label, color = colors.dangerText) },
                onClick = { open = false; onRemove() },
            )
        }
    }
}

@Composable
private fun RenameProfileDialog(dialog: RenameDialog, actions: ProfilesSectionActions) {
    val colors = RostrumTheme.colors
    val failure = (dialog.action as? ActionState.Failed)?.error
    AlertDialog(
        onDismissRequest = actions::dismissDialog,
        containerColor = colors.raised,
        titleContentColor = colors.text,
        shape = RoundedCornerShape(28.dp),
        title = { Text("Rename profile", style = RostrumText.sheetTitle) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                RostrumTextField(
                    value = dialog.text,
                    onValueChange = actions::onRenameChange,
                    placeholder = "Profile name",
                    accessibilityLabel = "Profile name",
                    isError = failure != null,
                )
                failure?.let { FieldError(it.describe()) }
            }
        },
        confirmButton = {
            TextPillButton(
                "Save",
                onClick = actions::saveRename,
                enabled = dialog.canSave,
                color = colors.accentText,
                style = RostrumText.button,
            )
        },
        dismissButton = { TextPillButton("Cancel", onClick = actions::dismissDialog) },
    )
}
