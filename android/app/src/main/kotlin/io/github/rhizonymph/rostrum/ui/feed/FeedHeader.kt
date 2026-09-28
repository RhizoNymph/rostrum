package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.ui.components.FilterPill
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The feed's 64dp header: wordmark and open count, the desktop pill, search
 * and filters (with the accent dot while any filter narrows the feed).
 */
@Composable
fun FeedHeader(
    openCount: Int?,
    desktop: PillView,
    filterActive: Boolean,
    filterCount: Int,
    searchOpen: Boolean,
    actions: FeedActions,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier.fillMaxWidth().height(64.dp).padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(
            modifier = Modifier.weight(1f),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("rostrum", style = RostrumText.screenTitle, color = colors.text, modifier = Modifier.semantics { heading() })
            if (openCount != null) {
                Text(openCountText(openCount), style = RostrumText.mono12, color = colors.textMuted, modifier = Modifier.padding(top = 3.dp))
            }
        }
        DesktopPillButton(desktop, actions.openDesktop, Modifier.padding(end = 2.dp))
        RostrumIconButton(
            icon = if (searchOpen) RostrumIcons.Close else RostrumIcons.Search,
            contentDescription = if (searchOpen) "Close search" else "Search pull requests",
            onClick = if (searchOpen) actions.closeSearch else actions.openSearch,
        )
        Box {
            RostrumIconButton(
                icon = RostrumIcons.Filter,
                contentDescription = filterButtonDescription(filterCount),
                onClick = actions.openFilters,
            )
            if (filterActive) {
                Box(
                    Modifier
                        .align(Alignment.TopEnd)
                        .padding(top = 11.dp, end = 11.dp)
                        .size(12.dp)
                        .clip(CircleShape)
                        .background(colors.bg)
                        .padding(2.dp)
                        .clip(CircleShape)
                        .background(colors.accent),
                )
            }
        }
    }
}

/** The 30dp pill with a status dot and the machine's name; the touch target is 44dp tall. */
@Composable
private fun DesktopPillButton(pill: PillView, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(15.dp)
    Box(
        modifier = modifier
            .height(48.dp)
            .clip(shape)
            .clickable(onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = pill.description
                role = Role.Button
            },
        contentAlignment = Alignment.Center,
    ) {
        Row(
            modifier = Modifier
                .height(30.dp)
                .clip(shape)
                .background(colors.surface)
                .border(1.dp, colors.border, shape)
                .padding(horizontal = 11.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(7.dp),
        ) {
            pill.dot?.let { StatusDot(it.colors().solid) }
            Text(
                pill.label,
                style = RostrumText.chip,
                color = if (pill.dot == null) colors.accentText else colors.textSecondary,
                maxLines = 1,
            )
        }
    }
}

/** The search field that opens under the header; typing narrows the feed after a pause. */
@Composable
fun FeedSearchField(text: String, onChange: (String) -> Unit, modifier: Modifier = Modifier) {
    val focus = remember { FocusRequester() }
    val focusManager = LocalFocusManager.current
    LaunchedEffect(Unit) { focus.requestFocus() }
    Row(
        modifier = modifier.fillMaxWidth().padding(start = 12.dp, end = 4.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RostrumTextField(
            value = text,
            onValueChange = onChange,
            placeholder = "Title, number, author or label",
            accessibilityLabel = "Search pull requests",
            modifier = Modifier.weight(1f).focusRequester(focus),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { focusManager.clearFocus() }),
        )
        if (text.isNotEmpty()) {
            RostrumIconButton(RostrumIcons.Close, "Clear search", onClick = { onChange("") }, iconSize = 18.dp)
        }
    }
}

/** Authors · Involved · Drafts, under the header. */
@Composable
fun FilterChipsRow(
    preferences: FeedPreferences,
    viewerLogin: String?,
    actions: FeedActions,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(start = 12.dp, end = 12.dp, top = 2.dp, bottom = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        FilterPill(
            text = authorsChipLabel(preferences, viewerLogin),
            selected = preferences.authors.isNotEmpty(),
            onClick = actions.openFilters,
        )
        FilterPill("Involved", selected = preferences.includeInvolved, onClick = actions.toggleInvolved)
        FilterPill("Drafts", selected = !preferences.hideDrafts, onClick = actions.toggleDrafts)
    }
}
