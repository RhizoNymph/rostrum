package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/** Settings › Desktop › "Copy settings from <machine>". */
@OptIn(ExperimentalCoroutinesApi::class)
class DesktopConfigSheetViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private class Harness(
        val backend: FakeRostrumBackend,
        val vm: DesktopConfigSheetViewModel,
        val messages: MutableList<String>,
        val copied: MutableList<Unit>,
    )

    private fun TestScope.harness(backend: FakeRostrumBackend = testBackend()): Harness {
        val vm = DesktopConfigSheetViewModel(backend)
        val messages = mutableListOf<String>()
        val copied = mutableListOf<Unit>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.toList(messages) }
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.copied.toList(copied) }
        return Harness(backend, vm, messages, copied)
    }

    @Test
    fun `the sheet starts closed and opening loads the preview`() = runTest(main.dispatcher) {
        val h = harness()
        assertEquals(CopySheetState.Closed, h.vm.state.value)
        h.vm.open()
        assertEquals(CopySheetState.Loading, h.vm.state.value)
        advanceUntilIdle()
        val ready = h.vm.state.value as CopySheetState.Ready
        assertEquals("nymph-desk", ready.preview.machine)
        assertTrue(ready.preview.changesAnything)
        assertEquals("2 repositories will be removed from this phone.", ready.removalWarning)
        assertEquals(ActionState.Idle, ready.copy)
    }

    @Test
    fun `replacing copies, refreshes the feed, says so and closes`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.open()
        advanceUntilIdle()
        val refreshes = h.backend.feedRefreshes
        h.vm.replace()
        advanceUntilIdle()
        assertEquals(CopySheetState.Closed, h.vm.state.value)
        assertEquals(refreshes + 1, h.backend.feedRefreshes)
        assertEquals(listOf("Copied 4 repositories from nymph-desk"), h.messages)
        assertEquals(1, h.copied.size)
        assertEquals(4, h.backend.settings().orFail().repos.size)
    }

    @Test
    fun `when nothing would change, replacing does nothing`() = runTest(main.dispatcher) {
        val h = harness()
        h.backend.copyDesktopConfig().orFail()
        h.vm.open()
        advanceUntilIdle()
        val ready = h.vm.state.value as CopySheetState.Ready
        assertFalse(ready.preview.changesAnything)
        val refreshes = h.backend.feedRefreshes
        h.vm.replace()
        advanceUntilIdle()
        assertEquals(ready, h.vm.state.value)
        assertEquals(refreshes, h.backend.feedRefreshes)
        assertTrue(h.copied.isEmpty())
    }

    @Test
    fun `a failed preview shows its error and can be retried`() = runTest(main.dispatcher) {
        val h = harness()
        h.backend.failNext(FakeCall.DesktopConfig, BackendError.DesktopUnreachable("connection refused"))
        h.vm.open()
        advanceUntilIdle()
        assertEquals(CopySheetState.Failed(BackendError.DesktopUnreachable("connection refused")), h.vm.state.value)
        h.vm.retry()
        advanceUntilIdle()
        assertInstanceOf(CopySheetState.Ready::class.java, h.vm.state.value)
    }

    @Test
    fun `unpaired, the preview is NotPaired`() = runTest(main.dispatcher) {
        val h = harness(testBackend(paired = false))
        h.vm.open()
        advanceUntilIdle()
        assertEquals(CopySheetState.Failed(BackendError.NotPaired), h.vm.state.value)
    }

    @Test
    fun `a failed copy keeps the sheet open with the error, and nothing changes`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.open()
        advanceUntilIdle()
        val before = h.backend.settings().orFail()
        val error = BackendError.RemoteApi(RemoteErrorCode.Busy, "a sync is running")
        h.backend.failNext(FakeCall.CopyDesktopConfig, error)
        h.vm.replace()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(error), (h.vm.state.value as CopySheetState.Ready).copy)
        assertEquals(before, h.backend.settings().orFail())
        assertTrue(h.messages.isEmpty())
        h.vm.dismiss()
        assertEquals(CopySheetState.Closed, h.vm.state.value)
    }

    @Test
    fun `dismissing while loading closes and ignores the late answer`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.open()
        h.vm.dismiss()
        advanceUntilIdle()
        assertEquals(CopySheetState.Closed, h.vm.state.value)
    }

    @Test
    fun `settings reload after a copy shows the desktop's repositories`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val settings = SettingsViewModel(backend, restoredSession(backend), onNotificationSettingsChanged = {})
        advanceUntilIdle()
        val h = harness(backend)
        h.vm.open()
        advanceUntilIdle()
        h.vm.replace()
        advanceUntilIdle()
        settings.refreshContent()
        advanceUntilIdle()
        val content = (settings.state.value.content as UiState.Loaded).data
        assertEquals(
            listOf("RhizoNymph/rostrum", "zed-industries/zed", "tokio-rs/tokio", "serde-rs/serde"),
            content.repos.map { it.repo },
        )
    }
}
