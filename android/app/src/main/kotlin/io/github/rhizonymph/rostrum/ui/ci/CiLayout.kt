package io.github.rhizonymph.rostrum.ui.ci

import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.CiSection
import io.github.rhizonymph.rostrum.data.model.RepoLoad

/** Narrowing the core's grid to one repository, and the words for a section without rows. */
object CiGridLayout {
    /** Only [repo]'s section, its lines re-indexed to section 0; the whole grid when [repo] is null. */
    fun only(grid: CiGrid, repo: String?): CiGrid {
        if (repo == null) return grid
        val index = grid.sections.indexOfFirst { it.repo.equals(repo, ignoreCase = true) }
        if (index < 0) return grid.copy(sections = emptyList(), lines = emptyList())
        val lines = grid.lines.mapNotNull { line ->
            when (line) {
                is CiLine.Header -> line.takeIf { it.section == index }?.let { CiLine.Header(0) }
                is CiLine.Stack -> line.takeIf { it.section == index }?.let { CiLine.Stack(0, it.members) }
                is CiLine.Row -> line.takeIf { it.section == index }?.let { CiLine.Row(0, it.row) }
                is CiLine.Notice -> line.takeIf { it.section == index }?.let { CiLine.Notice(0) }
                CiLine.Spacer -> null
            }
        }
        return grid.copy(sections = listOf(grid.sections[index]), lines = lines)
    }

    fun notice(section: CiSection, filter: CiGridFilter): String = when (val load = section.load) {
        RepoLoad.Idle -> "Checks not fetched yet"
        RepoLoad.Loading -> "Fetching checks…"
        is RepoLoad.Failed -> "Couldn't fetch checks: ${load.reason}"
        is RepoLoad.Loaded -> if (filter.needsAttention && section.hidden > 0) {
            "Nothing needs attention · ${section.hidden} hidden"
        } else {
            "No open pull requests"
        }
    }
}

/** Sentences of the CI screen. */
object CiText {
    fun cellDescription(number: Int, column: CiColumn, cell: CiCell?): String {
        val state = when {
            cell == null -> "not run"
            cell.timingLabel != null -> "${cell.statusLabel}, ${cell.timingLabel}"
            else -> cell.statusLabel
        }
        return "${column.label} on #$number: $state"
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
