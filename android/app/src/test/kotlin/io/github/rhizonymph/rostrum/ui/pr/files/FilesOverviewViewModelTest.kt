package io.github.rhizonymph.rostrum.ui.pr.files

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class FilesOverviewViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val pr = PrRef("RhizoNymph/rostrum", 10)

    @Test
    fun `starts loading, then shows the overview`() = runTest(main.dispatcher) {
        val vm = FilesOverviewViewModel(testBackend(), pr)
        assertEquals(UiState.Loading, vm.state.value)
        advanceUntilIdle()
        val overview = (vm.state.value as UiState.Loaded).data
        assertEquals(7, overview.stats.files)
        assertEquals(1, vm.firstRankedFile())
    }

    @Test
    fun `a failed load shows the error and retry recovers`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.FilesOverview, BackendError.Network("offline"))
        val vm = FilesOverviewViewModel(backend, pr)
        advanceUntilIdle()
        assertEquals(UiState.Error(BackendError.Network("offline")), vm.state.value)
        assertEquals(null, vm.firstRankedFile())
        vm.retry()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value)
    }

    @Test
    fun `refreshing picks up new drafts without going back to loading`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = FilesOverviewViewModel(backend, pr)
        advanceUntilIdle()
        backend.addDraft(pr, CommentAnchor("crates/rostrum-diff/src/overview.rs", 55, Side.Right), null, "x")
        vm.refresh()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value)
        advanceUntilIdle()
        val file = (vm.state.value as UiState.Loaded).data.files[1]
        assertEquals(2, file.drafts)
    }

    @Test
    fun `a failed refresh keeps the overview on screen`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = FilesOverviewViewModel(backend, pr)
        advanceUntilIdle()
        backend.failNext(FakeCall.FilesOverview, BackendError.Network("offline"))
        vm.refresh()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value)
    }
}
