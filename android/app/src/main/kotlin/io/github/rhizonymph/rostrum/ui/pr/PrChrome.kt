package io.github.rhizonymph.rostrum.ui.pr

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TitleStack
import io.github.rhizonymph.rostrum.ui.navigation.PrTab
import io.github.rhizonymph.rostrum.ui.pr.common.stateBadge
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** What the header's overflow menu can do; wired to the ViewModel by the route. */
data class PrMenuActions(
    val onToggleDraft: () -> Unit,
    val onRequestClose: () -> Unit,
    val onRequestReopen: () -> Unit,
    val onRefresh: () -> Unit,
)

/**
 * Back, the repository over `#10` and its state chip, then Open in browser
 * and the overflow menu. [header] is null until the detail has loaded.
 */
@Composable
fun PrTopBar(
    pr: PrRef,
    header: PullHeader?,
    busy: PrBusy?,
    onBack: () -> Unit,
    actions: PrMenuActions,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val uriHandler = LocalUriHandler.current
    var menuOpen by remember { mutableStateOf(false) }
    BackTopBar(
        onBack = onBack,
        backDescription = "Back to feed",
        modifier = modifier,
        actions = {
            RostrumIconButton(
                RostrumIcons.OpenInBrowser,
                "Open in browser",
                onClick = { uriHandler.openUri(header?.url ?: "https://github.com/${pr.repo}/pull/${pr.number}") },
            )
            Box {
                RostrumIconButton(RostrumIcons.MoreVert, "More actions", onClick = { menuOpen = true })
                DropdownMenu(
                    expanded = menuOpen,
                    onDismissRequest = { menuOpen = false },
                    containerColor = colors.raised,
                    shape = RoundedCornerShape(12.dp),
                ) {
                    val itemColors = MenuDefaults.itemColors(textColor = colors.text, disabledTextColor = colors.textSubtle)
                    fun item(label: String, enabled: Boolean, action: () -> Unit): @Composable () -> Unit = {
                        DropdownMenuItem(
                            text = { Text(label, style = RostrumText.label) },
                            onClick = { menuOpen = false; action() },
                            enabled = enabled,
                            colors = itemColors,
                        )
                    }
                    if (header != null && header.state == PullState.Open) {
                        item(header.draftAction.label, busy == null, actions.onToggleDraft)()
                        item("Close pull request…", busy == null, actions.onRequestClose)()
                    }
                    if (header != null && header.state == PullState.Closed) {
                        item("Reopen pull request…", busy == null, actions.onRequestReopen)()
                    }
                    item("Refresh", true, actions.onRefresh)()
                }
            }
        },
    ) {
        TitleStack(overline = pr.repo) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("#${pr.number}", style = RostrumText.monoNumber16, color = colors.text)
                if (header != null) {
                    val badge = stateBadge(header.state, header.isDraft)
                    StatusChip(badge.text, badge.role)
                }
            }
        }
    }
}

/** The 48dp four-column tab row with the 40×3 accent indicator. */
@Composable
fun PrTabRow(
    selected: PrTab,
    filesCount: Int?,
    onSelect: (PrTab) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().height(47.dp)) {
            PrTab.entries.forEach { tab ->
                val isSelected = tab == selected
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .selectable(selected = isSelected, role = Role.Tab, onClick = { onSelect(tab) }),
                    contentAlignment = Alignment.Center,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(
                            tab.title,
                            style = RostrumText.label.copy(fontWeight = if (isSelected) FontWeight.SemiBold else FontWeight.Medium),
                            color = if (isSelected) colors.text else colors.textMuted,
                            maxLines = 1,
                        )
                        if (tab == PrTab.Files && filesCount != null) {
                            Text(filesCount.toString(), style = RostrumText.mono12, color = if (isSelected) colors.text else colors.textMuted)
                        }
                    }
                    if (isSelected) {
                        Box(
                            Modifier
                                .align(Alignment.BottomCenter)
                                .size(width = 40.dp, height = 3.dp)
                                .clip(RoundedCornerShape(topStart = 3.dp, topEnd = 3.dp))
                                .background(colors.accent),
                        )
                    }
                }
            }
        }
        CardDivider()
    }
}
