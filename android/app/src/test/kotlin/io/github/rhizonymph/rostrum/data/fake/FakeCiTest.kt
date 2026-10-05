package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiNotRerunnable
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiRollupState
import io.github.rhizonymph.rostrum.data.model.CiSource
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
        val section = sections.single { it.repo == repo }
        val index = section.columns.indexOfFirst { it.label == column }
        return section.rows.single { it.number == number }.cells[index]
    }

    @Test
    fun `before a fetch every row is unfetched with no cells`() = runTest {
        val grid = testBackend().ciGrid(all).orFail()
        val rostrum = grid.sections.first { it.repo == SamplePulls.ROSTRUM }
        assertEquals(RepoLoad.Idle, rostrum.load)
        assertTrue(rostrum.rows.isNotEmpty())
        assertTrue(rostrum.rows.all { !it.fetched && it.cells.all { cell -> cell == null } })
        assertFalse(grid.ticks)
        assertFalse(grid.anyRunning)
    }

    @Test
    fun `a fetch fills the cells, in the feed's repository order with a header per section`() = runTest {
        val grid = testBackend().refreshCi(all).orFail()
        assertEquals(CiStatus.Failure, grid.cell(SamplePulls.ROSTRUM, 10, "CI / test")?.status)
        assertEquals(CiStatus.InProgress, grid.cell(SamplePulls.ROSTRUM, 9, "CI / clippy")?.status)
        assertEquals(CiStatus.Queued, grid.cell(SamplePulls.ROSTRUM, 9, "Coverage")?.status)
        assertEquals(CiStatus.Skipped, grid.cell(SamplePulls.ROSTRUM, 11, "deploy/preview")?.status)
        assertTrue(grid.ticks)
        assertTrue(grid.anyRunning)
        assertEquals(CiLine.Header(0), grid.lines.first())
        assertEquals(grid.sections.size, grid.lines.count { it is CiLine.Header })
        val rostrum = grid.sections.first { it.repo == SamplePulls.ROSTRUM }
        assertEquals(CiRollupState.Failing, rostrum.rows.single { it.number == 10 }.rollup.state)
        assertEquals("1 failing", rostrum.rows.single { it.number == 10 }.rollup.label)
        assertEquals("2 running", rostrum.rows.single { it.number == 9 }.rollup.label)
    }

    @Test
    fun `needs attention keeps failing and running rows and counts the rest as hidden`() = runTest {
        val backend = testBackend()
        backend.refreshCi(all).orFail()
        val grid = backend.ciGrid(CiGridFilter(needsAttention = true)).orFail()
        val rostrum = grid.sections.first { it.repo == SamplePulls.ROSTRUM }
        assertEquals(listOf(10, 9), rostrum.rows.map { it.number })
        assertEquals(1, rostrum.hidden)
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
        val columns = grid.sections.first { it.repo == SamplePulls.ROSTRUM }.columns
        fun key(label: String) = columns.single { it.label == label }.key

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
        assertInstanceOf(RepoLoad.Loaded::class.java, grid.sections.single { it.repo == SamplePulls.ZED }.load)
        assertEquals(RepoLoad.Idle, grid.sections.single { it.repo == SamplePulls.ROSTRUM }.load)
    }
}
