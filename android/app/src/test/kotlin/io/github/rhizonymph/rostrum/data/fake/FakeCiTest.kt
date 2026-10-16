package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiColumnGroup
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiNotRerunnable
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiRollupState
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.data.model.CiStackPlace
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** The fake's CI grid behaves like the core's: nothing until fetched, labels from the clock, typed re-runs. */
class FakeCiTest {
    private val all = CiGridFilter()

    private fun CiGrid.cell(repo: String, number: Int, column: String): CiCell? {
        val index = columns.indexOfFirst { it.label == column }
        return rows.single { it.repo == repo && it.number == number }.cells[index]
    }

    @Test
    fun `before a fetch every row is unfetched with no cells and there are no columns`() = runTest {
        val grid = testBackend().ciGrid(all).orFail()
        assertEquals(RepoLoad.Idle, grid.repos.first { it.repo == SamplePulls.ROSTRUM }.load)
        assertTrue(grid.rows.any { it.repo == SamplePulls.ROSTRUM })
        assertTrue(grid.rows.all { !it.fetched && it.cells.isEmpty() })
        assertTrue(grid.columns.isEmpty())
        assertFalse(grid.ticks)
        assertFalse(grid.anyRunning)
    }

    @Test
    fun `a fetch fills one matrix across repositories, in the feed's order`() = runTest {
        val grid = testBackend().refreshCi(all).orFail()
        assertEquals(CiStatus.Failure, grid.cell(SamplePulls.ROSTRUM, 10, "CI / test")?.status)
        assertEquals(CiStatus.InProgress, grid.cell(SamplePulls.ROSTRUM, 9, "CI / clippy")?.status)
        assertEquals(CiStatus.Queued, grid.cell(SamplePulls.ROSTRUM, 9, "Coverage")?.status)
        assertEquals(CiStatus.Skipped, grid.cell(SamplePulls.ROSTRUM, 10, "deploy/preview")?.status)
        assertNull(grid.cell(SamplePulls.ROSTRUM, 9, "deploy/preview"))
        assertNull(grid.cell(SamplePulls.ZED, 38150, "Coverage"))
        assertEquals(CiStatus.Failure, grid.cell(SamplePulls.ZED, 38150, "CI / test")?.status)
        assertTrue(grid.ticks)
        assertTrue(grid.anyRunning)
        // Columns: no workflow first, then by workflow and name; groups partition them.
        assertEquals(listOf("Coverage", "deploy/preview", "CI / build", "CI / clippy", "CI / test"), grid.columns.map { it.label })
        assertEquals(listOf(CiColumnGroup(null, 0, 2), CiColumnGroup("CI", 2, 5)), grid.groups)
        assertTrue(grid.rows.all { it.cells.size == grid.columns.size })
        // Rows follow the watched repositories' order; drafts are never rows.
        assertEquals(grid.repos.map { it.repo }.filter { r -> grid.rows.any { it.repo == r } }, grid.rows.map { it.repo }.distinct())
        assertFalse(grid.rows.any { it.repo == SamplePulls.ROSTRUM && it.number == 11 })
        assertEquals(grid.rows.indices.toList(), grid.lines.filterIsInstance<CiLine.Row>().map { it.row })
        val rostrum = grid.rows.filter { it.repo == SamplePulls.ROSTRUM }
        assertEquals(CiRollupState.Failing, rostrum.single { it.number == 10 }.rollup.state)
        assertEquals("1 failing", rostrum.single { it.number == 10 }.rollup.label)
        assertEquals("2 running", rostrum.single { it.number == 9 }.rollup.label)
        // #9 heads a stack whose other member (#11) is a draft.
        assertEquals(CiStackPlace.Only, rostrum.single { it.number == 9 }.stack)
        val nine = grid.rows.indexOfFirst { it.repo == SamplePulls.ROSTRUM && it.number == 9 }
        val at = grid.lines.indexOf(CiLine.Row(nine))
        assertEquals(CiLine.Stack(1), grid.lines[at - 1])
        assertEquals(grid.rows.size, grid.repos.sumOf { it.rows })
    }

    @Test
    fun `needs attention keeps failing and running rows, columns unchanged, and counts the rest as hidden`() = runTest {
        val backend = testBackend()
        val everything = backend.refreshCi(all).orFail()
        val grid = backend.ciGrid(CiGridFilter(needsAttention = true)).orFail()
        assertEquals(listOf(10, 9), grid.rows.filter { it.repo == SamplePulls.ROSTRUM }.map { it.number })
        assertEquals(everything.columns, grid.columns)
        assertEquals(everything.rows.size - grid.rows.size, grid.hidden)
        assertEquals(grid.hidden, grid.repos.sumOf { it.hidden })
        assertTrue(grid.hidden > 0)
        assertTrue(grid.rows.all { it.rollup.state == CiRollupState.Failing || it.rollup.state == CiRollupState.Running })
    }

    @Test
    fun `a failed job's log names its first error and is cut to its tail unless asked for in full`() = runTest {
        val backend = testBackend()
        val grid = backend.refreshCi(all).orFail()
        val source = grid.cell(SamplePulls.ROSTRUM, 10, "CI / test")!!.source as CiSource.Actions
        val tail = backend.jobLog(SamplePulls.ROSTRUM, source.jobId, full = false).orFail()
        assertTrue(tail.truncated)
        assertTrue(tail.dropped > 0)
        val error = checkNotNull(tail.firstError)
        assertTrue(tail.lines[error].text.contains("panicked"))
        assertNotNull(tail.failingStep)
        val full = backend.jobLog(SamplePulls.ROSTRUM, source.jobId, full = true).orFail()
        assertFalse(full.truncated)
        assertEquals(0, full.dropped)
        assertEquals(1, full.lines.first().number)
    }

    @Test
    fun `a passing job's log has no error`() = runTest {
        val backend = testBackend()
        val grid = backend.refreshCi(all).orFail()
        val source = grid.cell(SamplePulls.ROSTRUM, 10, "CI / build")!!.source as CiSource.Actions
        assertNull(backend.jobLog(SamplePulls.ROSTRUM, source.jobId, full = false).orFail().firstError)
    }

    @Test
    fun `re-runs follow the check's kind and state`() = runTest {
        val backend = testBackend()
        val grid = backend.refreshCi(all).orFail()
        fun key(label: String) = grid.columns.single { it.label == label }.key

        val failed = backend.rerunTargets(SamplePulls.ROSTRUM, 10, key("CI / test")).orFail() as CiRerunChoice.Available
        assertEquals(listOf("Re-run failed jobs", "Re-run this job", "Re-run all jobs"), failed.options.map { it.label })
        assertInstanceOf(CiRerun.FailedJobs::class.java, failed.options.first().rerun)

        val running = backend.rerunTargets(SamplePulls.ROSTRUM, 9, key("CI / clippy")).orFail()
        assertEquals(CiNotRerunnable.StillRunning, (running as CiRerunChoice.Unavailable).reason)

        val status = backend.rerunTargets(SamplePulls.ROSTRUM, 10, key("deploy/preview")).orFail()
        assertEquals(CiNotRerunnable.LegacyStatus, (status as CiRerunChoice.Unavailable).reason)

        val app = backend.rerunTargets(SamplePulls.ROSTRUM, 10, key("Coverage")).orFail() as CiRerunChoice.Available
        assertInstanceOf(CiRerun.Suite::class.java, app.options.single().rerun)
    }

    @Test
    fun `re-running a job flips it to queued, then a fetch runs it`() = runTest {
        val backend = testBackend()
        val grid = backend.refreshCi(all).orFail()
        val source = grid.cell(SamplePulls.ROSTRUM, 10, "CI / test")!!.source as CiSource.Actions
        backend.rerun(SamplePulls.ROSTRUM, CiRerun.Job(source.jobId)).orFail()
        assertEquals(CiStatus.Queued, backend.ciGrid(all).orFail().cell(SamplePulls.ROSTRUM, 10, "CI / test")?.status)
        assertEquals(CiStatus.InProgress, backend.refreshCi(all).orFail().cell(SamplePulls.ROSTRUM, 10, "CI / test")?.status)
        assertEquals(CiStatus.Success, backend.refreshCi(all).orFail().cell(SamplePulls.ROSTRUM, 10, "CI / test")?.status)
    }

    @Test
    fun `a zed job's log is found by its id`() = runTest {
        val backend = testBackend()
        val grid = backend.refreshCi(all).orFail()
        val source = grid.cell(SamplePulls.ZED, 38150, "CI / test")!!.source as CiSource.Actions
        assertNotNull(backend.jobLog(SamplePulls.ZED, source.jobId, full = false).orFail().firstError)
    }

    @Test
    fun `a re-run of nothing is CiNotFound, a not-run cell is invalid, and signed out is NotSignedIn`() = runTest {
        val backend = testBackend()
        backend.refreshCi(all).orFail()
        assertEquals(Outcome.Err(BackendError.CiNotFound), backend.rerun(SamplePulls.ROSTRUM, CiRerun.Job(42)))
        val unknown = backend.rerunTargets(SamplePulls.ROSTRUM, 999, io.github.rhizonymph.rostrum.data.model.CiCheckKey("CI", "build"))
        assertInstanceOf(BackendError.InvalidInput::class.java, (unknown as Outcome.Err).error)
        assertEquals(Outcome.Err(BackendError.NotSignedIn), testBackend(signedIn = false).ciGrid(all))
    }

    @Test
    fun `one repository can be fetched alone`() = runTest {
        val grid = testBackend().refreshCiRepo(SamplePulls.ZED, all).orFail()
        assertInstanceOf(RepoLoad.Loaded::class.java, grid.repos.single { it.repo == SamplePulls.ZED }.load)
        assertEquals(RepoLoad.Idle, grid.repos.single { it.repo == SamplePulls.ROSTRUM }.load)
        assertTrue(grid.rows.filter { it.repo == SamplePulls.ROSTRUM }.none { it.fetched })
        assertEquals(listOf("CI / build", "CI / test"), grid.columns.map { it.label })
    }
}
