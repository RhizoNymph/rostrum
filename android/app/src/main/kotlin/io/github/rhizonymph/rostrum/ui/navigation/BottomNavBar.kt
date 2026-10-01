package io.github.rhizonymph.rostrum.ui.navigation

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * Feed · Desktop · Settings. The Desktop item carries a warning badge with
 * the number of handoff sessions waiting ([desktopBadge], hidden at 0).
 */
@Composable
fun BottomNavBar(
    current: TopLevel?,
    desktopBadge: Int,
    onSelect: (TopLevel) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth().background(colors.surface).windowInsetsPadding(WindowInsets.navigationBars)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
        Row(Modifier.fillMaxWidth().height(79.dp)) {
            TopLevel.entries.forEach { item ->
                val selected = item == current
                val badge = if (item == TopLevel.Desktop) desktopBadge else 0
                val description = if (badge > 0) "${item.label}, $badge waiting" else item.label
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .selectable(selected = selected, role = Role.Tab, onClick = { onSelect(item) })
                        .semantics { contentDescription = description },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
                ) {
                    Box(
                        Modifier
                            .size(width = 64.dp, height = 32.dp)
                            .clip(RoundedCornerShape(16.dp))
                            .background(if (selected) colors.tonal else colors.surface),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = when (item) {
                                TopLevel.Feed -> RostrumIcons.Feed
                                TopLevel.Desktop -> RostrumIcons.Desktop
                                TopLevel.Settings -> RostrumIcons.Settings
                            },
                            contentDescription = null,
                            tint = if (selected) colors.onTonal else colors.textMuted,
                            modifier = Modifier.size(22.dp),
                        )
                        if (badge > 0) {
                            Box(
                                Modifier
                                    .align(Alignment.TopEnd)
                                    .offset(x = (-16).dp, y = 3.dp)
                                    .sizeIn(minWidth = 16.dp, minHeight = 16.dp)
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(colors.warning)
                                    .padding(horizontal = 4.dp),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(
                                    badge.toString(),
                                    style = RostrumText.mono11.copy(fontWeight = FontWeight.SemiBold, fontSize = RostrumText.mono11.fontSize * 0.91f),
                                    color = colors.bg,
                                )
                            }
                        }
                    }
                    Text(
                        item.label,
                        style = RostrumText.navLabel.copy(fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium),
                        color = if (selected) colors.text else colors.textMuted,
                    )
                }
            }
        }
    }
}
