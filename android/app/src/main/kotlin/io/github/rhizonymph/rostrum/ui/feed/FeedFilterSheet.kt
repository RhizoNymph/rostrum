package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.AuthorChip
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.CheckboxVisual
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.SwitchRow
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The "Filter feed" sheet: authors, involvement, drafts, empty repositories. */
@Composable
fun FeedFilterSheet(
    preferences: FeedPreferences,
    sheet: FilterSheetState.Open,
    actions: FilterSheetActions,
) {
    RostrumBottomSheet(onDismiss = actions.done) {
        FeedFilterContent(preferences, sheet, actions)
    }
}

/** The sheet's content, separate from the sheet so it previews. */
@Composable
fun FeedFilterContent(
    preferences: FeedPreferences,
    sheet: FilterSheetState.Open,
    actions: FilterSheetActions,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 12.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                "Filter feed",
                style = RostrumText.sheetTitle.copy(lineHeight = RostrumText.screenTitle.lineHeight),
                color = colors.text,
                modifier = Modifier.weight(1f).semantics { heading() },
            )
            activeText(activeFilterCount(preferences))?.let {
                Text(it, style = RostrumText.mono12, color = colors.accentText)
            }
        }
        Column(
            Modifier
                .weight(1f, fill = false)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 12.dp),
        ) {
            SheetSection("Authors", trailing = "most recent first", topPadding = 12)
            AuthorList(sheet, actions)
            Box(Modifier.padding(horizontal = 12.dp, vertical = 4.dp).fillMaxWidth().height(1.dp).background(colors.border))
            SwitchRow(
                title = "Include involved",
                subtitle = "Also show pull requests they're assigned to or asked to review",
                checked = preferences.includeInvolved,
                onCheckedChange = actions.setIncludeInvolved,
                horizontalPadding = 12.dp,
            )
            SheetSection("Feed", topPadding = 16)
            SwitchRow(
                title = "Show drafts",
                checked = !preferences.hideDrafts,
                onCheckedChange = actions.setShowDrafts,
                horizontalPadding = 12.dp,
            )
            SwitchRow(
                title = "Hide empty repositories",
                subtitle = "Loading and failed repositories always stay visible",
                checked = preferences.hideEmptyRepos,
                onCheckedChange = actions.setHideEmpty,
                horizontalPadding = 12.dp,
            )
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
        Column(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "These settings are saved. Search is not.",
                style = RostrumText.caption,
                color = colors.textSubtle,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextPillButton("Clear", actions.clear)
                Spacer(Modifier.weight(1f))
                PrimaryButton("Done", actions.done)
            }
        }
    }
}

@Composable
private fun SheetSection(title: String, trailing: String? = null, topPadding: Int) {
    val colors = RostrumTheme.colors
    Row(
        Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = topPadding.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = RostrumText.section, color = colors.textMuted, modifier = Modifier.weight(1f).semantics { heading() })
        if (trailing != null) Text(trailing, style = RostrumText.caption, color = colors.textSubtle)
    }
}

@Composable
private fun AuthorList(sheet: FilterSheetState.Open, actions: FilterSheetActions) {
    when (val roster = sheet.roster) {
        UiState.Loading -> LoadingView()
        is UiState.Error -> ErrorView(roster.error, title = "Couldn't load authors", onRetry = actions.retryRoster)
        is UiState.Loaded -> AuthorRows(roster.data, sheet.expanded, actions)
    }
}

@Composable
private fun AuthorRows(roster: AuthorRoster, expanded: Boolean, actions: FilterSheetActions) {
    val colors = RostrumTheme.colors
    roster.authors.forEach { author -> AuthorRow(author, onToggle = { actions.toggleAuthor(author.login) }) }
    if (!expanded && roster.hidden > 0) {
        TextPillButton(
            text = "Show all ${roster.authors.size + roster.hidden} authors",
            onClick = actions.showAllAuthors,
            trailing = {
                Icon(
                    RostrumIcons.ChevronDown,
                    contentDescription = null,
                    tint = colors.accentText,
                    modifier = Modifier.padding(start = 6.dp).size(16.dp),
                )
            },
        )
    }
}

@Composable
private fun AuthorRow(author: AuthorChip, onToggle: () -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clip(RoundedCornerShape(12.dp))
            .toggleable(value = author.selected, role = Role.Checkbox, onValueChange = { onToggle() })
            .padding(horizontal = 12.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Avatar(author.login, size = 32.dp)
        Column(Modifier.weight(1f)) {
            Text(author.login, style = RostrumText.rowTitle, color = colors.text, maxLines = 1)
            Text(authorSubline(author.isViewer, author.openPrs), style = RostrumText.caption, color = colors.textMuted)
        }
        CheckboxVisual(author.selected)
    }
}
