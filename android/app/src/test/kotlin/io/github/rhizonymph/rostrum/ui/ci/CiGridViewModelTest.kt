package io.github.rhizonymph.rostrum.ui.ci

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.fake.SamplePulls
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiNotRerunnable
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

@OptIn(ExperimentalCoroutinesApi::class)
class CiGridViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private inner class Harness(val backend: FakeRostrumBackend, val vm: CiGridViewModel, val messages: MutableList<String>) {
        val grid: CiGrid get() = (vm.state.value.content as UiState.Loaded).data
        val sheet: CiSheet get() = checkNotNull(vm.state.value.sheet)

        /** Open the cell of pull request [number] in the column labelled [column]. */
        fun open(repo: String, number: Int, column: String) {
            val section = grid.sections.indexOfFirst { it.repo == repo }
            val row = grid.sections[section].rows.indexOfFirst { it.number == number }
            vm.openCell(section, row, grid.sections[section].columns.indexOfFirst { it.label == column })
        }

        fun status(repo: String, number: Int, column: String): CiStatus? {
            val section = grid.sections.single { it.repo == repo }
            return section.rows.single { it.number == number }.cells[section.columns.indexOfFirst { it.label == column }]?.status
        }
    }

    private fun TestScope.harness(backend: FakeRostrumBackend = testBackend(), repo: String? = null): Harness {
        val vm = CiGridViewModel(backend, repo)
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.toList(messages) }
        return Harness(backend, vm, messages)
    }

    @Nested
    inner class Loading {
        @Test
        fun `opening shows the held grid, then fetches`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            assertEquals(1, h.backend.ci.fetches)
            assertFalse(h.vm.state.value.refreshing)
            assertEquals(CiStatus.Failure, h.status(SamplePulls.ROSTRUM, 10, "CI / test"))
        }

        @Test
        fun `a repository's grid shows only that repository and fetches only it`() = runTest(main.dispatcher) {
            val h = harness(repo = SamplePulls.ZED)
            advanceUntilIdle()
            assertEquals(listOf(SamplePulls.ZED), h.grid.sections.map { it.repo })
            assertEquals(1, h.backend.ci.fetches)
        }

        @Test
        fun `a failed fetch keeps the held grid and says why`() = runTest(main.dispatcher) {
            val backend = testBackend()
            backend.failNext(FakeCall.RefreshCi, BackendError.Network("offline"))
            val h = harness(backend)
            advanceUntilIdle()
            assertInstanceOf(UiState.Loaded::class.java, h.vm.state.value.content)
            assertEquals(listOf("Couldn't reach GitHub: offline"), h.messages)
        }

        @Test
        fun `a grid that cannot be built is an error`() = runTest(main.dispatcher) {
            val h = harness(testBackend(signedIn = false))
            advanceUntilIdle()
            assertEquals(UiState.Error(BackendError.NotSignedIn), h.vm.state.value.content)
        }

        @Test
        fun `needs attention narrows the grid without fetching`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.vm.setNeedsAttention(true)
            advanceUntilIdle()
            assertTrue(h.vm.state.value.filter.needsAttention)
            val rostrum = h.grid.sections.single { it.repo == SamplePulls.ROSTRUM }
            assertEquals(listOf(10, 9), rostrum.rows.map { it.number })
            assertEquals(1, h.backend.ci.fetches)
        }

        @Test
        fun `pull to refresh fetches again`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.vm.refresh()
            assertTrue(h.vm.state.value.refreshing)
            advanceUntilIdle()
            assertFalse(h.vm.state.value.refreshing)
            assertEquals(2, h.backend.ci.fetches)
        }
    }

    @Nested
    inner class Ticking {
        @Test
        fun `while shown and ticking, the grid is rebuilt every second and fetched every 15 s`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            assertTrue(h.grid.ticks)
            val builds = h.backend.ci.builds
            h.vm.start()
            advanceTimeBy(3_500)
            runCurrent()
            assertEquals(builds + 3, h.backend.ci.builds)
            assertEquals(1, h.backend.ci.fetches)
            advanceTimeBy(12_000)
            runCurrent()
            assertEquals(2, h.backend.ci.fetches)
            h.vm.stop()
            val stopped = h.backend.ci.builds
            advanceTimeBy(60_000)
            runCurrent()
            assertEquals(stopped, h.backend.ci.builds)
            assertEquals(2, h.backend.ci.fetches)
        }

        @Test
        fun `a settled grid neither ticks nor polls`() = runTest(main.dispatcher) {
            val h = harness(repo = SamplePulls.ZED)
            advanceUntilIdle()
            assertFalse(h.grid.ticks)
            assertFalse(h.grid.anyRunning)
            val builds = h.backend.ci.builds
            h.vm.start()
            advanceTimeBy(31_000)
            runCurrent()
            assertEquals(builds, h.backend.ci.builds)
            assertEquals(1, h.backend.ci.fetches)
            h.vm.stop()
        }
    }

    @Nested
    inner class Cells {
        @Test
        fun `an Actions cell opens its log, with the core's groups collapsed`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            assertEquals(10, h.sheet.target.number)
            advanceUntilIdle()
            val detail = h.sheet.detail as CiDetail.Log
            val log = (detail.log as UiState.Loaded).data
            assertEquals(log.collapsed.toSet(), detail.view.collapsed)
            assertTrue(log.truncated)
        }

        @Test
        fun `loading the full log replaces the tail`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            advanceUntilIdle()
            h.vm.loadFullLog()
            assertTrue((h.sheet.detail as CiDetail.Log).loadingFull)
            advanceUntilIdle()
            val detail = h.sheet.detail as CiDetail.Log
            assertFalse(detail.loadingFull)
            assertFalse((detail.log as UiState.Loaded).data.truncated)
        }

        @Test
        fun `groups toggle, search moves between matches and reveals them, and jumping finds the first error`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            advanceUntilIdle()
            val log = ((h.sheet.detail as CiDetail.Log).log as UiState.Loaded).data
            val first = log.collapsed.first()
            h.vm.toggleGroup(first)
            assertFalse(first in (h.sheet.detail as CiDetail.Log).view.collapsed)
            h.vm.toggleGroup(first)
            assertTrue(first in (h.sheet.detail as CiDetail.Log).view.collapsed)

            h.vm.setLogQuery("compiling")
            val searched = (h.sheet.detail as CiDetail.Log).view
            assertTrue(searched.matches.isNotEmpty())
            assertEquals(searched.matches.first(), searched.focus?.line)
            val group = LogLayout.groupOf(log, searched.matches.first())
            assertTrue(group == null || group !in searched.collapsed)
            h.vm.nextMatch()
            assertEquals(searched.matches[1], (h.sheet.detail as CiDetail.Log).view.focus?.line)
            h.vm.previousMatch()
            h.vm.previousMatch()
            assertEquals(searched.matches.last(), (h.sheet.detail as CiDetail.Log).view.focus?.line)

            h.vm.jumpToFirstError()
            assertEquals(log.firstError, (h.sheet.detail as CiDetail.Log).view.focus?.line)
        }

        @Test
        fun `an app's check opens its output and a legacy status only its link`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "Coverage")
            advanceUntilIdle()
            val output = (h.sheet.detail as CiDetail.Output).output as UiState.Loaded
            assertEquals(2, output.data.annotations.size)
            h.vm.closeSheet()
            assertNull(h.vm.state.value.sheet)
            h.open(SamplePulls.ROSTRUM, 10, "deploy/preview")
            assertEquals(CiDetail.Status, h.sheet.detail)
        }

        @Test
        fun `a cell outside the grid opens nothing`() = runTest(main.dispatcher) {
            val h = harness(repo = SamplePulls.ROSTRUM)
            advanceUntilIdle()
            h.vm.openCell(0, 99, 0)
            assertNull(h.vm.state.value.sheet)
        }
    }

    @Nested
    inner class Retry {
        @Test
        fun `retry offers the core's options, confirms, re-runs and shows the cell queued`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            advanceUntilIdle()
            h.vm.openRetry()
            advanceUntilIdle()
            val menu = h.sheet.retry as RetryState.Menu
            val options = (menu.choice as CiRerunChoice.Available).options
            assertEquals("Re-run failed jobs", options.first().label)
            h.vm.chooseRerun(options[1])
            assertEquals(RetryState.Confirm(options[1]), h.sheet.retry)
            h.vm.confirmRerun()
            advanceUntilIdle()
            assertNull(h.vm.state.value.sheet)
            assertEquals(listOf("Asked GitHub to re-run this job on #10"), h.messages)
            assertEquals(1, h.backend.ci.reruns)
            assertEquals(CiStatus.Queued, h.status(SamplePulls.ROSTRUM, 10, "CI / test"))
        }

        @Test
        fun `cancelling the confirmation re-runs nothing`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            advanceUntilIdle()
            h.vm.openRetry()
            advanceUntilIdle()
            val option = ((h.sheet.retry as RetryState.Menu).choice as CiRerunChoice.Available).options.first()
            h.vm.chooseRerun(option)
            h.vm.dismissRetry()
            advanceUntilIdle()
            assertEquals(RetryState.Closed, h.sheet.retry)
            assertEquals(0, h.backend.ci.reruns)
        }

        @Test
        fun `a running check says it cannot be re-run yet`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 9, "CI / clippy")
            advanceUntilIdle()
            h.vm.openRetry()
            advanceUntilIdle()
            val choice = (h.sheet.retry as RetryState.Menu).choice as CiRerunChoice.Unavailable
            assertEquals(CiNotRerunnable.StillRunning, choice.reason)
        }

        @Test
        fun `a refused re-run keeps the sheet with the typed reason`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            advanceUntilIdle()
            h.vm.openRetry()
            advanceUntilIdle()
            val option = ((h.sheet.retry as RetryState.Menu).choice as CiRerunChoice.Available).options.first()
            h.vm.chooseRerun(option)
            h.backend.failNext(FakeCall.Rerun, BackendError.CiNoPermission("the token lacks the workflow scope"))
            h.vm.confirmRerun()
            advanceUntilIdle()
            assertEquals(RetryState.Failed(BackendError.CiNoPermission("the token lacks the workflow scope")), h.sheet.retry)
            assertEquals(0, h.backend.ci.reruns)
            assertEquals(CiRerun.FailedJobs::class, option.rerun::class)
        }

        @Test
        fun `a failed lookup of the options is shown in place`() = runTest(main.dispatcher) {
            val h = harness()
            advanceUntilIdle()
            h.open(SamplePulls.ROSTRUM, 10, "CI / test")
            advanceUntilIdle()
            h.backend.failNext(FakeCall.RerunTargets, BackendError.CiNotFound)
            h.vm.openRetry()
            advanceUntilIdle()
            assertEquals(RetryState.Failed(BackendError.CiNotFound), h.sheet.retry)
        }
    }
}
