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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
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
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.ui.components.CiGlyph
import io.github.rhizonymph.rostrum.ui.components.CiShape
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The frozen left column (repository, number, title, rollup) and each check column's width. */
private val LeftWidth = 168.dp
private val CellWidth = 104.dp
private val RowHeight = 72.dp

fun CiStatus.shape(): CiShape = when (this) {
    CiStatus.Success -> CiShape.Passing
    CiStatus.Failure, CiStatus.TimedOut, CiStatus.ActionRequired -> CiShape.Failing
    CiStatus.Queued, CiStatus.InProgress -> CiShape.Running
    CiStatus.Cancelled, CiStatus.Skipped, CiStatus.Neutral -> CiShape.Skipped
}

/**
 * The pull requests × checks matrix of every repository: one header (a
 * workflow band over the job names) stays above the list, and it and every
 * row's cells share one horizontal scroll, so they move together while the
 * left column stays put. A repository's first row is ruled off from the one
 * before. With no rows, the empty state says why.
 */
@Composable
fun CiGridTable(
    grid: CiGrid,
    filter: CiGridFilter,
    onOpenCell: (row: Int, column: Int) -> Unit,
    onOpenPullRequest: (repo: String, number: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val scroll = rememberScrollState()
    val failures = CiGridLayout.failures(grid)
    Column(modifier.fillMaxSize()) {
        if (failures.isNotEmpty()) FailureStrip(failures)
        if (grid.rows.isEmpty()) {
            Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp)) {
                Text(CiGridLayout.emptyMessage(grid, filter), style = RostrumText.body, color = RostrumTheme.colors.textMuted)
            }
            return@Column
        }
        MatrixHeader(grid, scroll)
        val starts = remember(grid) { CiGridLayout.repoStarts(grid) }
        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(grid.lines, key = { index, line -> lineKey(grid, index, line) }) { index, line ->
                val startsRepo = index in starts
                when (line) {
                    is CiLine.Stack -> StackLine(line.members, startsRepo)
                    is CiLine.Row -> {
                        val row = grid.rows[line.row]
                        PullRow(
                            grid = grid,
                            row = row,
                            startsRepo = startsRepo,
                            scroll = scroll,
                            onOpenCell = { column -> onOpenCell(line.row, column) },
                            onOpenPullRequest = { onOpenPullRequest(row.repo, row.number) },
                        )
                    }
                }
            }
            CiText.hiddenFooter(grid.hidden)?.let { footer ->
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
}

private fun lineKey(grid: CiGrid, index: Int, line: CiLine): Any = when (line) {
    is CiLine.Row -> grid.rows[line.row].let { "r:${it.repo}#${it.number}" }
    is CiLine.Stack -> "s:$index"
}

/** Repositories whose checks failed to load, one line each. */
@Composable
private fun FailureStrip(failures: List<CiRepoFailure>) {
    val colors = RostrumTheme.colors
    Column(
        Modifier
            .fillMaxWidth()
            .background(colors.danger.copy(alpha = 0.10f))
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        failures.forEach {
            Text(it.text, style = RostrumText.caption, color = colors.dangerText, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** The one header: a workflow band over each group's columns, then the job names. */
@Composable
private fun MatrixHeader(grid: CiGrid, scroll: ScrollState) {
    val colors = RostrumTheme.colors
    Column(Modifier.fillMaxWidth().background(colors.bg).semantics { heading() }) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
            Text(
                "Pull request",
                style = RostrumText.caption,
                color = colors.textMuted,
                modifier = Modifier.width(LeftWidth).padding(start = 16.dp, bottom = 6.dp),
            )
            Column(Modifier.horizontalScroll(scroll)) {
                Row {
                    grid.groups.forEach { group ->
                        val span = (group.end - group.start).coerceAtLeast(0)
                        Box(Modifier.width(CellWidth * span).padding(horizontal = 3.dp, vertical = 2.dp)) {
                            if (group.workflow != null) {
                                Text(
                                    group.workflow,
                                    style = RostrumText.caption.copy(fontWeight = FontWeight.SemiBold),
                                    color = colors.textSecondary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(colors.raised)
                                        .padding(horizontal = 6.dp, vertical = 2.dp),
                                )
                            }
                        }
                    }
                }
                Row(Modifier.height(36.dp), verticalAlignment = Alignment.CenterVertically) {
                    grid.columns.forEach { column ->
                        Text(
                            column.key.name,
                            style = RostrumText.caption,
                            color = colors.textSecondary,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.width(CellWidth).padding(horizontal = 6.dp),
                        )
                    }
                }
            }
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
    }
}

/** A line above a repository's first row: the rule that sets it off from the rows before. */
@Composable
private fun RepoRule(startsRepo: Boolean) {
    if (startsRepo) Box(Modifier.fillMaxWidth().height(2.dp).background(RostrumTheme.colors.borderStrong))
}

@Composable
private fun StackLine(members: Int, startsRepo: Boolean) {
    Column {
        RepoRule(startsRepo)
        Text(
            "Stack · $members PRs",
            style = RostrumText.caption,
            color = RostrumTheme.colors.accentText,
            modifier = Modifier.padding(start = 16.dp, top = 6.dp, bottom = 2.dp),
        )
    }
}

@Composable
private fun PullRow(
    grid: CiGrid,
    row: CiRow,
    startsRepo: Boolean,
    scroll: ScrollState,
    onOpenCell: (Int) -> Unit,
    onOpenPullRequest: () -> Unit,
) {
    val colors = RostrumTheme.colors
    Column {
        RepoRule(startsRepo)
        Row(Modifier.fillMaxWidth().height(RowHeight), verticalAlignment = Alignment.CenterVertically) {
            Row(
                Modifier
                    .width(LeftWidth)
                    .fillMaxHeight()
                    .clickable(onClickLabel = "Open pull request", onClick = onOpenPullRequest)
                    .clearAndSetSemantics {
                        contentDescription = "${CiText.rowLabel(row.repo, row.number)} ${row.title}, ${row.rollup.label}"
                        role = Role.Button
                    },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier
                        .padding(start = 8.dp, end = 6.dp)
                        .width(3.dp)
                        .fillMaxHeight(0.7f)
                        .background(if (row.stack != null) colors.accent else Color.Transparent, RoundedCornerShape(2.dp)),
                )
                Column(Modifier.padding(end = 6.dp), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Row {
                        Text(
                            row.repo,
                            style = RostrumText.mono11,
                            color = colors.textMuted,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        Text(" #${row.number}", style = RostrumText.mono11, color = colors.textSecondary, maxLines = 1)
                    }
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
                        description = CiText.cellDescription(row.repo, row.number, grid.columns[index], cell),
                        onClick = { onOpenCell(index) },
                    )
                }
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
