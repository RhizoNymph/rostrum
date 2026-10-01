package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.TabCounts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The tab's name. */
fun tabTitle(tab: FeedTab): String = when (tab) {
    FeedTab.PullRequests -> "Pull requests"
    FeedTab.Issues -> "Issues"
}

/** For screen readers: `Pull requests, 6 open`. */
fun tabDescription(tab: FeedTab, counts: TabCounts): String = "${tabTitle(tab)}, ${counts.of(tab)} open"

/**
 * Pull requests | Issues, each with its count, and the sort summary
 * (`pushed ↓ · created ↓`), which opens the Sort sheet too.
 */
@Composable
fun FeedTabs(tab: FeedTab, counts: TabCounts, sortSummary: String, actions: FeedActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier.fillMaxWidth().padding(start = 8.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FeedTab.entries.forEach { entry ->
            val selected = entry == tab
            Column(
                modifier = Modifier
                    .heightIn(min = 48.dp)
                    .selectable(selected = selected, role = Role.Tab, onClick = { actions.selectTab(entry) })
                    .semantics { contentDescription = tabDescription(entry, counts) }
                    .padding(horizontal = 8.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        tabTitle(entry),
                        style = RostrumText.label.copy(fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium),
                        color = if (selected) colors.text else colors.textMuted,
                    )
                    Text(counts.of(entry).toString(), style = RostrumText.mono12, color = if (selected) colors.accentText else colors.textSubtle)
                }
                Box(
                    Modifier
                        .padding(top = 6.dp)
                        .width(40.dp)
                        .height(2.dp)
                        .background(if (selected) colors.accent else colors.bg),
                )
            }
        }
        Box(Modifier.weight(1f))
        Text(
            sortSummary,
            style = RostrumText.mono12,
            color = colors.textMuted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier
                .heightIn(min = 48.dp)
                .clickable(role = Role.Button, onClick = actions.openSort)
                .semantics { contentDescription = "Sort: $sortSummary" }
                .padding(start = 8.dp, top = 15.dp),
        )
    }
}
