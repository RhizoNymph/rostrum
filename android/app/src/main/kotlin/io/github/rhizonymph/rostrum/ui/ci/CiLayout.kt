package io.github.rhizonymph.rostrum.ui.ci

import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiColumnGroup
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.RepoLoad

/** Narrowing the core's matrix to one repository, and what the screen says around it. */
object CiGridLayout {
    /**
     * Only [repo]'s rows (re-indexed, with their stack lines) and the columns
     * some of them ran, regrouped; the whole grid when [repo] is null.
     */
    fun only(grid: CiGrid, repo: String?): CiGrid {
        if (repo == null) return grid
        val kept = grid.rows.indices.filter { grid.rows[it].repo.equals(repo, ignoreCase = true) }
        val newIndex = kept.withIndex().associate { (new, old) -> old to new }
        val columns = grid.columns.indices.filter { column -> kept.any { grid.rows[it].cells.getOrNull(column) != null } }
        val lines = buildList {
            var pendingStack: CiLine.Stack? = null
            grid.lines.forEach { line ->
                when (line) {
                    is CiLine.Stack -> pendingStack = line
                    is CiLine.Row -> {
                        val row = newIndex[line.row]
                        if (row != null) {
                            pendingStack?.let { add(it) }
                            add(CiLine.Row(row))
                        }
                        pendingStack = null
                    }
                }
            }
        }
        val repos = grid.repos.filter { it.repo.equals(repo, ignoreCase = true) }
        return grid.copy(
            columns = columns.map { grid.columns[it] },
            groups = regroup(grid.groups, columns),
            rows = kept.map { old -> grid.rows[old].let { r -> r.copy(cells = columns.map { r.cells[it] }) } },
            repos = repos,
            lines = lines,
            hidden = repos.sumOf { it.hidden },
        )
    }

    /** [groups] over only the columns at [kept] (old indices, ascending), adjacent survivors of one group merged. */
    fun regroup(groups: List<CiColumnGroup>, kept: List<Int>): List<CiColumnGroup> {
        val result = mutableListOf<CiColumnGroup>()
        var previous = -1
        kept.forEachIndexed { new, old ->
            val group = groups.indexOfFirst { old >= it.start && old < it.end }
            val last = result.lastOrNull()
            if (last != null && group == previous) {
                result[result.lastIndex] = last.copy(end = new + 1)
            } else {
                result += CiColumnGroup(groups.getOrNull(group)?.workflow, new, new + 1)
            }
            previous = group
        }
        return result
    }

    /** Indices of the lines that begin another repository's rows (a stack line begins its rows); never the first line. */
    fun repoStarts(grid: CiGrid): Set<Int> {
        val starts = mutableSetOf<Int>()
        var previous: String? = null
        var pending: Int? = null
        grid.lines.forEachIndexed { index, line ->
            when (line) {
                is CiLine.Stack -> if (pending == null) pending = index
                is CiLine.Row -> {
                    val repo = grid.rows.getOrNull(line.row)?.repo
                    val first = pending ?: index
                    if (previous != null && repo != previous) starts += first
                    previous = repo
                    pending = null
                }
            }
        }
        return starts
    }

    /** Repositories whose checks failed to load, with GitHub's reason. */
    fun failures(grid: CiGrid): List<CiRepoFailure> = grid.repos.mapNotNull { repo ->
        (repo.load as? RepoLoad.Failed)?.let { CiRepoFailure(repo.repo, it.reason) }
    }

    /** What the grid says when it has no rows. */
    fun emptyMessage(grid: CiGrid, filter: CiGridFilter): String = when {
        filter.needsAttention && grid.hidden > 0 -> "Nothing failing or running"
        grid.repos.any { it.load == RepoLoad.Idle || it.load == RepoLoad.Loading } -> "Loading checks…"
        else -> "No open pull requests outside drafts"
    }
}

/** A repository whose checks failed to load. */
data class CiRepoFailure(val repo: String, val reason: String) {
    val text: String get() = "$repo: $reason"
}

/** Sentences of the CI screen. */
object CiText {
    /** `owner/name #12`. */
    fun rowLabel(repo: String, number: Int) = "$repo #$number"

    /** A cell of the matrix, naming its repository since rows of every repository share one grid. */
    fun cellDescription(repo: String, number: Int, column: CiColumn, cell: CiCell?): String =
        "${column.label} on ${rowLabel(repo, number)}: ${cellState(cell)}"

    fun cellDescription(number: Int, column: CiColumn, cell: CiCell?): String =
        "${column.label} on #$number: ${cellState(cell)}"

    private fun cellState(cell: CiCell?): String = when {
        cell == null -> "not run"
        cell.timingLabel != null -> "${cell.statusLabel}, ${cell.timingLabel}"
        else -> cell.statusLabel
    }

    fun rerunRequested(option: CiRerunOption, number: Int) =
        "Asked GitHub to ${option.label.replaceFirstChar { it.lowercase() }} on #$number"

    fun hiddenFooter(hidden: Int): String? = when {
        hidden <= 0 -> null
        hidden == 1 -> "1 pull request hidden by Needs attention"
        else -> "$hidden pull requests hidden by Needs attention"
    }
}

/** One row of the log viewer: a line, or a group's header (folded or not). */
sealed interface LogRow {
    data class Line(val index: Int) : LogRow

    data class Group(val group: Int, val collapsed: Boolean, val hidden: Int) : LogRow
}

/** The log viewer's rows from a parsed log and the folded groups, plus search and lookups. */
object LogLayout {
    fun rows(log: CiJobLog, collapsed: Set<Int>): List<LogRow> {
        val groupAt = log.groups.withIndex().associate { (index, group) -> group.header to index }
        val rows = ArrayList<LogRow>(log.lines.size)
        var line = 0
        while (line < log.lines.size) {
            val group = groupAt[line]
            if (group == null) {
                rows += LogRow.Line(line)
                line++
                continue
            }
            val range = log.groups[group]
            val folded = group in collapsed
            rows += LogRow.Group(group, folded, hidden = (range.end - range.header - 1).coerceAtLeast(0))
            line = if (folded) maxOf(range.end, line + 1) else line + 1
        }
        return rows
    }

    /** Indices of the lines containing [query], ignoring case; none for a blank query. */
    fun matches(log: CiJobLog, query: String): List<Int> {
        val needle = query.trim()
        if (needle.isEmpty()) return emptyList()
        return log.lines.indices.filter { log.lines[it].text.contains(needle, ignoreCase = true) }
    }

    /** The group whose header is [line] or which holds it. */
    fun groupOf(log: CiJobLog, line: Int): Int? =
        log.groups.indexOfFirst { line >= it.header && line < it.end }.takeIf { it >= 0 }

    /** The row showing [line]: its own, or its group's header row; `null` when folded away. */
    fun rowOf(rows: List<LogRow>, log: CiJobLog, line: Int): Int? {
        val index = rows.indexOfFirst {
            when (it) {
                is LogRow.Line -> it.index == line
                is LogRow.Group -> log.groups.getOrNull(it.group)?.header == line
            }
        }
        return index.takeIf { it >= 0 }
    }
}
