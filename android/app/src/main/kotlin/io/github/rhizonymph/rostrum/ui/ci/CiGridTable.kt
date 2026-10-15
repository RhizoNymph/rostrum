package io.github.rhizonymph.rostrum.ui.ci

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiRow
import io.github.rhizonymph.rostrum.data.model.CiSection
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.ui.components.CiGlyph
import io.github.rhizonymph.rostrum.ui.components.CiShape
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.format.splitRepo
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The frozen left column (number, title, rollup) and each check column's width. */
private val LeftWidth = 148.dp
private val CellWidth = 104.dp
private val RowHeight = 64.dp

fun CiStatus.shape(): CiShape = when (this) {
    CiStatus.Success -> CiShape.Passing
    CiStatus.Failure, CiStatus.TimedOut, CiStatus.ActionRequired -> CiShape.Failing
    CiStatus.Queued, CiStatus.InProgress -> CiShape.Running
    CiStatus.Cancelled, CiStatus.Skipped, CiStatus.Neutral -> CiShape.Skipped
}

/**
 * The pull requests × checks matrix, scrolling both ways: the list scrolls
 * vertically; within a repository's section, the column headers and every
 * row's cells share one horizontal scroll, so they move together while the
 * left column (number, title, rollup) stays put.
 */
@Composable
fun CiGridTable(
    grid: CiGrid,
    filter: CiGridFilter,
    onOpenCell: (section: Int, row: Int, column: Int) -> Unit,
    onOpenPullRequest: (repo: String, number: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val repos = grid.sections.map { it.repo }
    val scrolls = remember(repos) { repos.associateWith { ScrollState(0) } }
    val hidden = grid.sections.sumOf { it.hidden }
    LazyColumn(modifier.fillMaxSize()) {
        itemsIndexed(grid.lines, key = { index, line -> lineKey(grid, index, line) }) { _, line ->
            when (line) {
                is CiLine.Header -> {
                    val section = grid.sections[line.section]
                    SectionHeader(section, scrolls.getValue(section.repo))
                }
                is CiLine.Stack -> StackLine(line.members)
                is CiLine.Row -> {
                    val section = grid.sections[line.section]
                    PullRow(
                        section = section,
                        row = section.rows[line.row],
                        scroll = scrolls.getValue(section.repo),
                        onOpenCell = { column -> onOpenCell(line.section, line.row, column) },
                        onOpenPullRequest = { onOpenPullRequest(section.repo, section.rows[line.row].number) },
                    )
                }
                is CiLine.Notice -> Text(
                    CiGridLayout.notice(grid.sections[line.section], filter),
                    style = RostrumText.meta,
                    color = RostrumTheme.colors.textMuted,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp),
                )
                CiLine.Spacer -> Spacer(Modifier.height(18.dp))
            }
        }
        CiText.hiddenFooter(hidden)?.let { footer ->
            item(key = "hidden") {
                Text(
                    footer,
                    style = RostrumText.caption,
                    color = RostrumTheme.colors.textMuted,
                    modifier = Modifier.fillMaxWidth().padding(16.dp),
                )
            }
        }
        item(key = "end") { Spacer(Modifier.height(24.dp)) }
    }
}

private fun lineKey(grid: CiGrid, index: Int, line: CiLine): Any = when (line) {
    is CiLine.Header -> "h:${grid.sections[line.section].repo}"
    is CiLine.Row -> "r:${grid.sections[line.section].repo}#${grid.sections[line.section].rows[line.row].number}"
    else -> "l:$index"
}

@Composable
private fun SectionHeader(section: CiSection, scroll: ScrollState) {
    val colors = RostrumTheme.colors
    val (owner, name) = splitRepo(section.repo)
    Column(Modifier.fillMaxWidth().background(colors.bg)) {
        Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 10.dp, bottom = 6.dp).semantics { heading() }) {
            Text(owner, style = RostrumText.label, color = colors.textMuted, maxLines = 1)
            Text(name, style = RostrumText.label.copy(fontWeight = FontWeight.SemiBold), color = colors.text, maxLines = 1)
        }
        Row(Modifier.fillMaxWidth().height(36.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                "Pull request",
                style = RostrumText.caption,
                color = colors.textMuted,
                modifier = Modifier.width(LeftWidth).padding(start = 16.dp),
            )
            Row(Modifier.horizontalScroll(scroll)) {
                section.columns.forEach { column ->
                    Text(
                        column.label,
                        style = RostrumText.caption,
                        color = colors.textSecondary,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.width(CellWidth).padding(horizontal = 4.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun StackLine(members: Int) {
    Text(
        "Stack · $members PRs",
        style = RostrumText.caption,
        color = RostrumTheme.colors.accentText,
        modifier = Modifier.padding(start = 16.dp, top = 6.dp, bottom = 2.dp),
    )
}

@Composable
private fun PullRow(
    section: CiSection,
    row: CiRow,
    scroll: ScrollState,
    onOpenCell: (Int) -> Unit,
    onOpenPullRequest: () -> Unit,
) {
    val colors = RostrumTheme.colors
    Row(Modifier.fillMaxWidth().height(RowHeight), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier
                .width(LeftWidth)
                .fillMaxHeight()
                .clickable(onClickLabel = "Open pull request", onClick = onOpenPullRequest)
                .clearAndSetSemantics {
                    contentDescription = "#${row.number} ${row.title}, ${row.rollup.label}"
                    role = Role.Button
                },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .padding(start = 8.dp, end = 6.dp)
                    .width(3.dp)
                    .fillMaxHeight(0.7f)
                    .background(if (row.stack != null) colors.accent else androidx.compose.ui.graphics.Color.Transparent, RoundedCornerShape(2.dp)),
            )
            Column(Modifier.padding(end = 6.dp), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Text("#${row.number}", style = RostrumText.mono12, color = colors.textMuted, maxLines = 1)
                Text(row.title, style = RostrumText.caption, color = colors.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    if (row.truncated) "${row.rollup.label} · more" else row.rollup.label,
                    style = RostrumText.caption,
                    color = row.rollup.role.colors().text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Row(Modifier.horizontalScroll(scroll)) {
            row.cells.forEachIndexed { index, cell ->
                CellTile(
                    cell = cell,
                    description = CiText.cellDescription(row.number, section.columns[index], cell),
                    onClick = { onOpenCell(index) },
                )
            }
        }
    }
}

@Composable
private fun CellTile(cell: CiCell?, description: String, onClick: () -> Unit) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(10.dp)
    Box(Modifier.width(CellWidth).height(RowHeight).padding(3.dp)) {
        if (cell == null) {
            Box(
                Modifier
                    .fillMaxSize()
                    .clip(shape)
                    .background(colors.raised.copy(alpha = 0.4f))
                    .clearAndSetSemantics { contentDescription = description },
                contentAlignment = Alignment.Center,
            ) {
                Text("—", style = RostrumText.caption, color = colors.textSubtle)
            }
            return@Box
        }
        val tones = cell.role.colors()
        Column(
            Modifier
                .fillMaxSize()
                .clip(shape)
                .background(tones.tint)
                .clickable(onClick = onClick)
                .clearAndSetSemantics {
                    contentDescription = description
                    role = Role.Button
                }
                .padding(horizontal = 8.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(3.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                CiGlyph(cell.status.shape(), tones.solid, size = 14.dp, contentDescription = null)
                Text(cell.statusLabel, style = RostrumText.caption, color = tones.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Text(
                cell.timingLabel ?: cell.producer,
                style = RostrumText.mono11,
                color = colors.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
