package io.github.rhizonymph.rostrum.ui.desktop

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.settings.accountSecrets
import io.github.rhizonymph.rostrum.ui.settings.restoredSession
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class DesktopViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private suspend fun TestScope.viewModel(
        backend: FakeRostrumBackend = testBackend(),
        paired: Boolean = true,
    ): Pair<DesktopViewModel, SessionRepository> {
        val session = restoredSession(backend, accountSecrets(paired = paired))
        val vm = DesktopViewModel(backend, session, TEST_CLOCK, pollMillis = 1_000)
        advanceUntilIdle()
        return vm to session
    }

    private fun DesktopViewModel.content(): DesktopContent =
        (state.value.page as? DesktopPage.Loaded)?.content ?: throw AssertionError("not loaded: ${state.value.page}")

    @Test
    fun `an unpaired phone asks to pair`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel(testBackend(paired = false), paired = false)
        assertEquals(DesktopPage.NotPaired, vm.state.value.page)
    }

    @Test
    fun `loads the machine, handoffs, stash preference and last run`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val content = vm.content()
        assertEquals("nymph-desk", content.machine.name)
        val handoffs = (content.handoffs as UiState.Loaded).data
        assertEquals("rostrum-RhizoNymph-rostrum-10", handoffs.single().session)
        assertFalse(content.autostash)
        assertEquals(SyncActivity.Idle, content.sync)
        assertEquals(LocalOp.RebaseBase, content.lastRun!!.op)
    }

    @Test
    fun `an unreachable desktop can be retried`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.MachineInfo, BackendError.DesktopUnreachable("no route"))
        val (vm, _) = viewModel(backend)
        assertEquals(DesktopPage.Unreachable(BackendError.DesktopUnreachable("no route")), vm.state.value.page)
        vm.refresh()
        advanceUntilIdle()
        assertInstanceOf(DesktopPage.Loaded::class.java, vm.state.value.page)
    }

    @Test
    fun `failing handoffs do not hide the rest`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.Handoffs, BackendError.DesktopTimeout)
        val (vm, _) = viewModel(backend)
        assertEquals(UiState.Error(BackendError.DesktopTimeout), vm.content().handoffs)
    }

    @Test
    fun `aborting a handoff removes it`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val session = (vm.content().handoffs as UiState.Loaded).data.single()
        val message = backgroundScope.launch { assertEquals("Aborted the rebase in feat/diff-overview", vm.messages.flow.first()) }
        vm.abort(session)
        assertTrue(session.session in vm.content().aborting)
        advanceUntilIdle()
        assertTrue((vm.content().handoffs as UiState.Loaded).data.isEmpty())
        assertTrue(vm.content().aborting.isEmpty())
        message.join()
    }

    @Test
    fun `a failed abort keeps the session and says why`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val (vm, _) = viewModel(backend)
        val session = (vm.content().handoffs as UiState.Loaded).data.single()
        backend.failNext(FakeCall.AbortLocal, BackendError.DesktopTimeout)
        vm.abort(session)
        advanceUntilIdle()
        assertEquals(1, (vm.content().handoffs as UiState.Loaded).data.size)
    }

    @Test
    fun `sync all runs, polls to completion and becomes the last run`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        vm.startSync(SyncAllOp.MergeBase)
        runCurrent()
        val running = vm.content().sync
        assertInstanceOf(SyncActivity.Running::class.java, running)
        advanceUntilIdle()
        val content = vm.content()
        assertEquals(SyncActivity.Idle, content.sync)
        assertEquals(LocalOp.MergeBase, content.lastRun!!.op)
        assertFalse(content.lastRun!!.running)
        assertEquals(2, (content.handoffs as UiState.Loaded).data.size)
    }

    @Test
    fun `a sync that cannot start returns to idle`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val (vm, _) = viewModel(backend)
        backend.failNext(FakeCall.StartSyncAll, BackendError.DesktopTimeout)
        vm.startSync(SyncAllOp.Pull)
        advanceUntilIdle()
        assertEquals(SyncActivity.Idle, vm.content().sync)
        assertEquals(LocalOp.RebaseBase, vm.content().lastRun!!.op)
    }

    @Test
    fun `a run already in progress is resumed on load`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.startSyncAll(SyncAllOp.Pull, autostash = false).orFail()
        val session = restoredSession(backend, accountSecrets())
        val vm = DesktopViewModel(backend, session, TEST_CLOCK, pollMillis = 1_000)
        runCurrent()
        assertInstanceOf(SyncActivity.Running::class.java, vm.content().sync)
        advanceUntilIdle()
        assertEquals(LocalOp.PullRebase, vm.content().lastRun!!.op)
    }

    @Test
    fun `the stash switch is saved`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val (vm, _) = viewModel(backend)
        vm.setAutostash(true)
        advanceUntilIdle()
        assertTrue(vm.content().autostash)
        assertTrue(backend.settings().orFail().autostash)
    }

    @Test
    fun `unpairing returns to the pair prompt`() = runTest(main.dispatcher) {
        val (vm, session) = viewModel()
        vm.unpair()
        advanceUntilIdle()
        assertEquals(DesktopLink.NotPaired, (session.state.value as SessionState.Ready).desktop)
        assertEquals(DesktopPage.NotPaired, vm.state.value.page)
    }

    @Test
    fun `an unpair the desktop does not answer offers to forget locally`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val (vm, _) = viewModel(backend)
        backend.failNext(FakeCall.Unpair, BackendError.DesktopUnreachable("offline"))
        vm.unpair()
        advanceUntilIdle()
        assertEquals(BackendError.DesktopUnreachable("offline"), vm.state.value.unpairFailure)
        vm.forgetDesktop()
        advanceUntilIdle()
        assertNull(vm.state.value.unpairFailure)
        assertEquals(DesktopPage.NotPaired, vm.state.value.page)
    }

    @Test
    fun `the desktop's token can be fetched`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val message = backgroundScope.launch { assertEquals("Signed in with nymph-desk's GitHub token", vm.messages.flow.first()) }
        vm.fetchGitHubToken()
        advanceUntilIdle()
        message.join()
    }
}
