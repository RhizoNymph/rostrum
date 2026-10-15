package io.github.rhizonymph.rostrum.ui.ci

import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiColumnGroup
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiLineKind
import io.github.rhizonymph.rostrum.data.model.CiLogGroup
import io.github.rhizonymph.rostrum.data.model.CiLogLine
import io.github.rhizonymph.rostrum.data.model.CiRepo
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.CiRollup
import io.github.rhizonymph.rostrum.data.model.CiRollupState
import io.github.rhizonymph.rostrum.data.model.CiRow
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.data.model.CiStackPlace
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class CiLayoutTest {
    private val loaded = RepoLoad.Loaded(TEST_NOW)

    private fun cell(status: CiStatus) = CiCell(
        status = status, statusLabel = status.name.lowercase(), role = ColorRole.Neutral,
        timingLabel = null, durationLabel = null, ticks = false,
        producer = "GitHub Actions", detailsUrl = null, source = CiSource.Actions(1, 2, 1),
    )

    private fun row(repo: String, number: Int, vararg cells: CiCell?, stack: CiStackPlace? = null) = CiRow(
        repo = repo, number = number, title = "PR $number", headSha = "abc1234",
        rollup = CiRollup(0, 0, 0, 0, CiRollupState.Empty, "no checks", ColorRole.Neutral),
        cells = cells.toList(), stack = stack, fetched = true, truncated = false,
    )

    private fun grid(repos: List<CiRepo>, rows: List<CiRow> = emptyList(), lines: List<CiLine> = emptyList(), hidden: Int = 0) = CiGrid(
        columns = emptyList(), groups = emptyList(), rows = rows, repos = repos, lines = lines,
        hidden = hidden, ticks = false, anyRunning = false,
    )

    @Nested
    inner class Grid {
        private val ok = cell(CiStatus.Success)

        // Columns: Coverage | CI / build, CI / test | Lint / fmt
        private val grid = CiGrid(
            columns = listOf(
                CiColumn(CiCheckKey(null, "Coverage"), "Coverage"),
                CiColumn(CiCheckKey("CI", "build"), "CI / build"),
                CiColumn(CiCheckKey("CI", "test"), "CI / test"),
                CiColumn(CiCheckKey("Lint", "fmt"), "Lint / fmt"),
            ),
            groups = listOf(CiColumnGroup(null, 0, 1), CiColumnGroup("CI", 1, 3), CiColumnGroup("Lint", 3, 4)),
            rows = listOf(
                row("a/one", 1, ok, null, null, ok),
                row("b/two", 7, null, ok, null, null, stack = CiStackPlace.Bottom),
                row("b/two", 8, null, null, null, ok, stack = CiStackPlace.Top),
                row("b/two", 3, null, ok, null, null),
                row("c/three", 2, ok, ok, ok, ok),
            ),
            repos = listOf(
                CiRepo("a/one", loaded, 1, 0),
                CiRepo("b/two", loaded, 3, 2),
                CiRepo("c/three", RepoLoad.Failed("502", TEST_NOW), 1, 1),
            ),
            lines = listOf(
                CiLine.Row(0), CiLine.Stack(2), CiLine.Row(1), CiLine.Row(2), CiLine.Row(3), CiLine.Row(4),
            ),
            hidden = 3,
            ticks = true,
            anyRunning = false,
        )

        @Test
        fun `without a repository the grid is unchanged`() {
            assertEquals(grid, CiGridLayout.only(grid, null))
        }

        @Test
        fun `one repository keeps its rows re-indexed with their stack, the columns they ran, regrouped`() {
            val only = CiGridLayout.only(grid, "b/two")
            assertEquals(listOf(7, 8, 3), only.rows.map { it.number })
            assertEquals(listOf(CiLine.Stack(2), CiLine.Row(0), CiLine.Row(1), CiLine.Row(2)), only.lines)
            assertEquals(listOf("CI / build", "Lint / fmt"), only.columns.map { it.label })
            assertEquals(listOf(CiColumnGroup("CI", 0, 1), CiColumnGroup("Lint", 1, 2)), only.groups)
            assertEquals(listOf(listOf(ok, null), listOf(null, ok), listOf(ok, null)), only.rows.map { it.cells })
            assertEquals(listOf("b/two"), only.repos.map { it.repo })
            assertEquals(2, only.hidden)
            assertEquals(grid.ticks, only.ticks)
        }

        @Test
        fun `a repository the grid lacks leaves it empty`() {
            val only = CiGridLayout.only(grid, "z/none")
            assertEquals(emptyList<CiRow>(), only.rows)
            assertEquals(emptyList<CiLine>(), only.lines)
            assertEquals(emptyList<CiColumn>(), only.columns)
            assertEquals(emptyList<CiRepo>(), only.repos)
        }

        @Test
        fun `regrouping merges adjacent survivors of one group`() {
            assertEquals(
                listOf(CiColumnGroup(null, 0, 1), CiColumnGroup("CI", 1, 3)),
                CiGridLayout.regroup(grid.groups, listOf(0, 1, 2)),
            )
            assertEquals(emptyList<CiColumnGroup>(), CiGridLayout.regroup(grid.groups, emptyList()))
        }

        @Test
        fun `a repository's first line is ruled off, a stack line before its rows, never the first line`() {
            assertEquals(setOf(1, 5), CiGridLayout.repoStarts(grid))
        }

        @Test
        fun `failed repositories are listed with their reason`() {
            assertEquals(listOf(CiRepoFailure("c/three", "502")), CiGridLayout.failures(grid))
            assertEquals("c/three: 502", CiGridLayout.failures(grid).single().text)
        }

        @Test
        fun `an empty grid says why`() {
            val all = CiGridFilter()
            val attention = CiGridFilter(needsAttention = true)
            assertEquals("Loading checks…", CiGridLayout.emptyMessage(grid(listOf(CiRepo("a/b", RepoLoad.Idle, 0, 0))), all))
            assertEquals("Loading checks…", CiGridLayout.emptyMessage(grid(listOf(CiRepo("a/b", RepoLoad.Loading, 0, 0))), all))
            assertEquals("No open pull requests outside drafts", CiGridLayout.emptyMessage(grid(listOf(CiRepo("a/b", loaded, 0, 0))), all))
            assertEquals(
                "Nothing failing or running",
                CiGridLayout.emptyMessage(grid(listOf(CiRepo("a/b", loaded, 0, 3)), hidden = 3), attention),
            )
            assertEquals("No open pull requests outside drafts", CiGridLayout.emptyMessage(grid(listOf(CiRepo("a/b", loaded, 0, 0))), attention))
        }
    }

    @Nested
    inner class Text {
        private val column = CiColumn(CiCheckKey("CI", "test"), "CI / test")
        private val cell = CiCell(
            status = CiStatus.Failure, statusLabel = "failure", role = ColorRole.Danger,
            timingLabel = "finished 12m ago", durationLabel = "took 4m 03s", ticks = false,
            producer = "GitHub Actions", detailsUrl = null, source = CiSource.Actions(1, 2, 1),
        )

        @Test
        fun `a matrix cell names its repository`() {
            assertEquals("CI / test on a/b #10: failure, finished 12m ago", CiText.cellDescription("a/b", 10, column, cell))
            assertEquals("a/b #10", CiText.rowLabel("a/b", 10))
        }

        @Test
        fun `a cell is described by its column, status and timing`() {
            assertEquals("CI / test on #10: failure, finished 12m ago", CiText.cellDescription(10, column, cell))
            assertEquals("CI / test on #10: not run", CiText.cellDescription(10, column, null))
            assertEquals("CI / test on #10: failure", CiText.cellDescription(10, column, cell.copy(timingLabel = null)))
        }

        @Test
        fun `a requested re-run is confirmed in a sentence`() {
            val option = CiRerunOption(CiRerun.FailedJobs(2), "Re-run failed jobs", "Re-run the failed jobs?")
            assertEquals("Asked GitHub to re-run failed jobs on #10", CiText.rerunRequested(option, 10))
        }

        @Test
        fun `hidden rows are counted`() {
            assertNull(CiText.hiddenFooter(0))
            assertEquals("1 pull request hidden by Needs attention", CiText.hiddenFooter(1))
            assertEquals("4 pull requests hidden by Needs attention", CiText.hiddenFooter(4))
        }
    }

    @Nested
    inner class Log {
        // 0 header A, 1-2 in A, 3 header B, 4-6 in B (5 is the error), 7 header C, 8 in C.
        private val log = CiJobLog(
            lines = listOf(
                CiLogLine(1, "Set up job", CiLineKind.GroupHeader),
                CiLogLine(2, "runner 2.3", CiLineKind.Plain),
                CiLogLine(3, "Ubuntu", CiLineKind.Plain),
                CiLogLine(4, "Run cargo test", CiLineKind.GroupHeader),
                CiLogLine(5, "running 2 tests", CiLineKind.Plain),
                CiLogLine(6, "error: assertion failed", CiLineKind.Error),
                CiLogLine(7, "test result: FAILED", CiLineKind.Plain),
                CiLogLine(8, "Post job cleanup", CiLineKind.GroupHeader),
                CiLogLine(9, "Cleaning up", CiLineKind.Plain),
            ),
            groups = listOf(CiLogGroup("Set up job", 0, 3), CiLogGroup("Run cargo test", 3, 7), CiLogGroup("Post job cleanup", 7, 9)),
            steps = emptyList(),
            firstError = 5,
            failingStep = null,
            collapsed = listOf(0, 2),
            dropped = 0,
            truncated = false,
        )

        @Test
        fun `collapsed groups show only their header with a count`() {
            assertEquals(
                listOf(
                    LogRow.Group(0, collapsed = true, hidden = 2),
                    LogRow.Group(1, collapsed = false, hidden = 3),
                    LogRow.Line(4), LogRow.Line(5), LogRow.Line(6),
                    LogRow.Group(2, collapsed = true, hidden = 1),
                ),
                LogLayout.rows(log, setOf(0, 2)),
            )
        }

        @Test
        fun `with nothing collapsed every line shows`() {
            val rows = LogLayout.rows(log, emptySet())
            assertEquals(9, rows.size)
            assertEquals(LogRow.Line(1), rows[1])
        }

        @Test
        fun `search matches case-insensitively, and blank matches nothing`() {
            assertEquals(listOf(5, 6), LogLayout.matches(log, "FAIL"))
            assertEquals(emptyList<Int>(), LogLayout.matches(log, "  "))
        }

        @Test
        fun `a line's group and row are found`() {
            assertEquals(1, LogLayout.groupOf(log, 5))
            assertEquals(1, LogLayout.groupOf(log, 3))
            assertEquals(0, LogLayout.groupOf(log, 2))
            val rows = LogLayout.rows(log, setOf(0, 2))
            assertEquals(3, LogLayout.rowOf(rows, log, 5))
            assertEquals(1, LogLayout.rowOf(rows, log, 3))
            assertNull(LogLayout.rowOf(rows, log, 1))
        }
    }
}
