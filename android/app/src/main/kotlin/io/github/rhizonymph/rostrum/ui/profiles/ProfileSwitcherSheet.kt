package io.github.rhizonymph.rostrum.ui.profiles

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.TonalButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.components.pairingPageSentence

/**
 * The profile switcher, opened from the feed's profile pill and the Desktop
 * tab's header. Picking a profile switches to it; the actions add another.
 */
@Composable
fun ProfileSwitcherSheet(
    onDismiss: () -> Unit,
    onPairDesktop: () -> Unit,
    onAddTokenProfile: () -> Unit,
) {
    val viewModel = rostrumViewModel { container -> ProfileSwitcherViewModel(container.profiles, container.appMessages) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    RostrumBottomSheet(onDismiss = onDismiss) {
        ProfileSwitcherContent(
            state = state,
            onPick = { id -> if (!viewModel.switchTo(id)) onDismiss() },
            onPairDesktop = {
                onDismiss()
                onPairDesktop()
            },
            onAddTokenProfile = {
                onDismiss()
                onAddTokenProfile()
            },
        )
    }
}

@Composable
fun ProfileSwitcherContent(
    state: SwitcherUiState,
    onPick: (ProfileId) -> Unit,
    onPairDesktop: () -> Unit,
    onAddTokenProfile: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(
        modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .navigationBarsPadding()
            .padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text("Profiles", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.semantics { heading() })
        Text(
            "Each profile has its own repositories, filters, drafts and GitHub account. The feed shows the active one.",
            style = RostrumText.body,
            color = colors.textSecondary,
        )
        RostrumCard(Modifier.fillMaxWidth()) {
            state.rows.forEachIndexed { index, row ->
                if (index > 0) CardDivider()
                ProfileRowView(row, switching = state.switching == row.id, onClick = { onPick(row.id) })
            }
        }
        state.error?.let { FieldError("Couldn't switch: ${it.describe()}") }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            TonalButton("Pair another desktop", onPairDesktop, modifier = Modifier.fillMaxWidth())
            Text(
                pairingPageSentence(
                    "Open ",
                    " on that desktop's network and tap Open in Rostrum, or scan its QR code. " +
                        "Pair another desktop also lets you type its address and code.",
                ),
                style = RostrumText.caption.copy(lineHeight = RostrumText.meta.lineHeight),
                color = colors.textMuted,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
            TonalButton("Add a GitHub token profile", onAddTokenProfile, modifier = Modifier.fillMaxWidth())
        }
    }
}

/** Avatar, label, "machine · @login", and a check on the active profile. */
@Composable
fun ProfileRowView(
    row: ProfileRow,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    switching: Boolean = false,
    trailing: (@Composable () -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 60.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics { selected = row.active }
            .padding(start = 14.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(row.login ?: row.label, size = 32.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                row.label,
                style = RostrumText.rowTitle.copy(fontWeight = FontWeight.SemiBold),
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(row.detail, style = RostrumText.caption, color = colors.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        when {
            switching -> CircularProgressIndicator(Modifier.size(18.dp), color = colors.accent, strokeWidth = 2.dp)
            row.active -> Icon(RostrumIcons.Check, contentDescription = "Active", tint = colors.accentText, modifier = Modifier.size(20.dp))
        }
        trailing?.invoke()
    }
}
