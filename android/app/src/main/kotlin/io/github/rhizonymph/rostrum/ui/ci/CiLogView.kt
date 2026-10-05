package io.github.rhizonymph.rostrum.ui.ci

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLineKind
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * A job's log: monospace lines in a lazy list, collapsible groups (starting
 * from the core's choice), the failing step tinted, search with previous and
 * next, a jump to the first error, and "Load full log" when only the tail
 * was parsed.
 */
@Composable
fun CiLogView(detail: CiDetail.Log, log: CiJobLog, actions: CiGridActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val view = detail.view
    val rows = remember(log, view.collapsed) { LogLayout.rows(log, view.collapsed) }
    val failing = log.failingStep?.let { log.steps.getOrNull(it) }
    val currentMatch = view.matches.getOrNull(view.current)
    val list = rememberLazyListState()
    LaunchedEffect(view.focus) {
        val focus = view.focus ?: return@LaunchedEffect
        LogLayout.rowOf(rows, log, focus.line)?.let { list.animateScrollToItem((it - 2).coerceAtLeast(0)) }
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            RostrumTextField(
                value = view.query,
                onValueChange = actions::setLogQuery,
                placeholder = "Search the log",
                accessibilityLabel = "Search the log",
                modifier = Modifier.weight(1f),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            )
            if (view.query.isNotBlank()) {
                Text(
                    if (view.matches.isEmpty()) "0" else "${view.current + 1}/${view.matches.size}",
                    style = RostrumText.mono12,
                    color = colors.textMuted,
                    modifier = Modifier.padding(horizontal = 4.dp),
                )
                RostrumIconButton(RostrumIcons.ChevronUp, "Previous match", actions::previousMatch, enabled = view.matches.isNotEmpty(), iconSize = 18.dp)
                RostrumIconButton(RostrumIcons.ChevronDown, "Next match", actions::nextMatch, enabled = view.matches.isNotEmpty(), iconSize = 18.dp)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                failing?.let { "Failed in: ${it.title}" } ?: "${log.lines.size} lines",
                style = RostrumText.caption,
                color = if (failing != null) colors.dangerText else colors.textMuted,
                modifier = Modifier.weight(1f),
                maxLines = 1,
            )
            if (log.firstError != null) TextPillButton("First error", actions::jumpToFirstError)
        }
        if (log.truncated) {
            Row(
                Modifier.fillMaxWidth().background(colors.raised, RoundedCornerShape(10.dp)).padding(start = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Showing the last ${log.lines.size} lines (${log.dropped} earlier)",
                    style = RostrumText.caption,
                    color = colors.textSecondary,
                    modifier = Modifier.weight(1f),
                )
                TextPillButton(if (detail.loadingFull) "Loading…" else "Load full log", actions::loadFullLog, enabled = !detail.loadingFull)
            }
        }
        LazyColumn(
            state = list,
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
                .background(colors.bg, RoundedCornerShape(10.dp))
                .padding(vertical = 6.dp),
        ) {
            itemsIndexed(rows, key = { _, row -> rowKey(row) }) { _, row ->
                when (row) {
                    is LogRow.Group -> GroupRow(log, row, onToggle = { actions.toggleGroup(row.group) })
                    is LogRow.Line -> {
                        val line = log.lines[row.index]
                        val inFailing = failing != null && row.index in failing.start until failing.end
                        LineRow(line.number, line.text, line.kind, inFailing, highlighted = row.index == currentMatch)
                    }
                }
            }
        }
    }
}

private fun rowKey(row: LogRow): Any = when (row) {
    is LogRow.Group -> "g${row.group}"
    is LogRow.Line -> row.index
}

@Composable
private fun GroupRow(log: CiJobLog, row: LogRow.Group, onToggle: () -> Unit) {
    val colors = RostrumTheme.colors
    val title = log.groups[row.group].title
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 36.dp)
            .clickable(onClick = onToggle)
            .clearAndSetSemantics {
                contentDescription = "$title, ${row.hidden} lines"
                stateDescription = if (row.collapsed) "Collapsed" else "Expanded"
                role = Role.Button
            }
            .padding(horizontal = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Icon(
            if (row.collapsed) RostrumIcons.ChevronRight else RostrumIcons.ChevronDown,
            contentDescription = null,
            tint = colors.textMuted,
            modifier = Modifier.size(14.dp),
        )
        Text(title, style = RostrumText.mono12, color = colors.text, maxLines = 1, modifier = Modifier.weight(1f))
        if (row.collapsed) Text("${row.hidden}", style = RostrumText.mono11, color = colors.textSubtle)
    }
}

@Composable
private fun LineRow(number: Int, text: String, kind: CiLineKind, inFailingStep: Boolean, highlighted: Boolean) {
    val colors = RostrumTheme.colors
    val textColor = when (kind) {
        CiLineKind.Error -> colors.dangerText
        CiLineKind.Warning -> colors.warningText
        CiLineKind.Notice -> colors.accentText
        CiLineKind.Command, CiLineKind.Debug -> colors.textMuted
        CiLineKind.GroupHeader, CiLineKind.Plain -> colors.textSecondary
    }
    val background = when {
        highlighted -> colors.warning.copy(alpha = 0.25f)
        kind == CiLineKind.Error -> colors.danger.copy(alpha = 0.14f)
        inFailingStep -> colors.danger.copy(alpha = 0.06f)
        else -> Color.Transparent
    }
    Row(Modifier.fillMaxWidth().background(background).padding(horizontal = 6.dp, vertical = 1.dp)) {
        Text(
            number.toString(),
            style = RostrumText.mono11,
            color = colors.textSubtle,
            modifier = Modifier.width(40.dp),
            maxLines = 1,
        )
        Text(text, style = RostrumText.mono11, color = textColor)
    }
}
