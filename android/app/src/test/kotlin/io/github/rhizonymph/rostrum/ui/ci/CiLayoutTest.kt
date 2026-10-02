package io.github.rhizonymph.rostrum.ui.ci

import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiLineKind
import io.github.rhizonymph.rostrum.data.model.CiLogGroup
import io.github.rhizonymph.rostrum.data.model.CiLogLine
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.CiSection
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class CiLayoutTest {
    private fun section(repo: String, load: RepoLoad = RepoLoad.Loaded(TEST_NOW), hidden: Int = 0) =
        CiSection(repo, columns = emptyList(), rows = emptyList(), load = load, hidden = hidden)

    @Nested
    inner class Grid {
        private val grid = CiGrid(
            sections = listOf(section("a/one"), section("b/two"), section("c/three")),
            lines = listOf(
                CiLine.Header(0), CiLine.Notice(0), CiLine.Spacer,
                CiLine.Header(1), CiLine.Stack(1, 2), CiLine.Row(1, 0), CiLine.Row(1, 1), CiLine.Spacer,
                CiLine.Header(2), CiLine.Notice(2),
            ),
            ticks = true,
            anyRunning = false,
        )

        @Test
        fun `without a repository the grid is unchanged`() {
            assertEquals(grid, CiGridLayout.only(grid, null))
        }

        @Test
        fun `one repository keeps its section, re-indexed, without spacers`() {
            val only = CiGridLayout.only(grid, "b/two")
            assertEquals(listOf("b/two"), only.sections.map { it.repo })
            assertEquals(listOf(CiLine.Header(0), CiLine.Stack(0, 2), CiLine.Row(0, 0), CiLine.Row(0, 1)), only.lines)
            assertEquals(grid.ticks, only.ticks)
        }

        @Test
        fun `a repository the grid lacks leaves it empty`() {
            val only = CiGridLayout.only(grid, "z/none")
            assertEquals(emptyList<CiSection>(), only.sections)
            assertEquals(emptyList<CiLine>(), only.lines)
        }

        @Test
        fun `a section without rows says why`() {
            val all = CiGridFilter()
            assertEquals("Checks not fetched yet", CiGridLayout.notice(section("a/b", RepoLoad.Idle), all))
            assertEquals("Fetching checks…", CiGridLayout.notice(section("a/b", RepoLoad.Loading), all))
            assertEquals("Couldn't fetch checks: 502", CiGridLayout.notice(section("a/b", RepoLoad.Failed("502", TEST_NOW)), all))
            assertEquals("No open pull requests", CiGridLayout.notice(section("a/b"), all))
            assertEquals("Nothing needs attention · 3 hidden", CiGridLayout.notice(section("a/b", hidden = 3), CiGridFilter(needsAttention = true)))
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
