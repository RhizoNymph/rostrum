package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.model.SortOption
import io.github.rhizonymph.rostrum.data.model.SortSettings
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RadioVisual
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The direction toggle's two choices, named for [option]: `Newest first | Oldest first`. */
fun <K> directionChoices(option: SortOption<K>): List<SortDirection> =
    listOf(option.defaultDirection, option.defaultDirection.reversed)

/**
 * Sort: Repositories, then "Pull requests & issues" (one sort serves both
 * tabs). Each section is its keys as radio rows and a direction toggle named
 * by the chosen key; picking another key resets to its default direction.
 */
@Composable
fun FeedSortSheet(settings: SortSettings, actions: SortSheetActions) {
    RostrumBottomSheet(onDismiss = actions.done) {
        FeedSortContent(settings, actions)
    }
}

@Composable
fun FeedSortContent(settings: SortSettings, actions: SortSheetActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(
        modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .verticalScroll(rememberScrollState())
            .padding(start = 12.dp, end = 12.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Sort", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.weight(1f).semantics { heading() })
            Text(settings.summary, style = RostrumText.mono12, color = colors.accentText)
        }
        SortSection(
            title = "Repositories",
            options = settings.repoOptions,
            selected = settings.repoKey,
            direction = settings.repoDirection,
            onChoose = actions.chooseRepoKey,
            onDirection = actions.setRepoDirection,
        )
        SortSection(
            title = "Pull requests & issues",
            options = settings.itemOptions,
            selected = settings.itemKey,
            direction = settings.itemDirection,
            onChoose = actions.chooseItemKey,
            onDirection = actions.setItemDirection,
        )
        Row(Modifier.fillMaxWidth().padding(end = 4.dp), horizontalArrangement = Arrangement.End) {
            PrimaryButton("Done", actions.done)
        }
    }
}

@Composable
private fun <K> SortSection(
    title: String,
    options: List<SortOption<K>>,
    selected: K,
    direction: SortDirection,
    onChoose: (K) -> Unit,
    onDirection: (SortDirection) -> Unit,
) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        SectionHeader(title, modifier = Modifier.padding(horizontal = 8.dp))
        options.forEach { option ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .selectable(selected = option.key == selected, role = Role.RadioButton, onClick = { onChoose(option.key) })
                    .padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                RadioVisual(option.key == selected)
                Text(option.label, style = RostrumText.rowTitle, color = colors.text)
            }
        }
        options.firstOrNull { it.key == selected }?.let { option ->
            SegmentedToggle(
                options = directionChoices(option),
                selected = direction,
                onSelect = onDirection,
                label = { option.directionLabel(it) },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            )
        }
    }
}
