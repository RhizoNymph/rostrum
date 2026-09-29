package io.github.rhizonymph.rostrum.ui.pr.branch

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class BranchViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val diffOverview = PrRef("RhizoNymph/rostrum", 10)
    private val authorFilter = PrRef("RhizoNymph/rostrum", 9)

    private fun viewModel(pr: PrRef = diffOverview, backend: FakeRostrumBackend = testBackend()) =
        BranchViewModel(pr, backend).also { it.ensureLoaded() }

    private fun BranchViewModel.ready(): LocalCardState.Ready = state.value.local as LocalCardState.Ready

    private fun TestScope.messagesOf(vm: BranchViewModel): MutableList<String> {
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.toList(messages) }
        return messages
    }

    @Test
    fun `nothing loads until the tab is shown`() = runTest(main.dispatcher) {
        val vm = BranchViewModel(diffOverview, testBackend())
        advanceUntilIdle()
        assertEquals(LocalCardState.Idle, vm.state.value.local)
        vm.ensureLoaded()
        assertEquals(LocalCardState.Loading, vm.state.value.local)
        advanceUntilIdle()
        assertInstanceOf(LocalCardState.Ready::class.java, vm.state.value.local)
    }

    @Test
    fun `a handed-off rebase shows with its machine`() = runTest(main.dispatcher) {
        val vm = viewModel()
        advanceUntilIdle()
        val ready = vm.ready()
        assertEquals("nymph-desk", ready.machine)
        val branch = (ready.status as LocalStatus.CheckedOut).branch
        assertNotNull(branch.inProgress)
        assertTrue(branch.handoff!!.running)
    }

    @Test
    fun `an unpaired phone says so`() = runTest(main.dispatcher) {
        val vm = viewModel(backend = testBackend(paired = false))
        advanceUntilIdle()
        assertEquals(LocalCardState.NotPaired, vm.state.value.local)
    }

    @Test
    fun `branches without a worktree or a clone`() = runTest(main.dispatcher) {
        val notCheckedOut = viewModel(PrRef("RhizoNymph/rostrum", 11))
        val notConfigured = viewModel(PrRef("rust-lang/rust", 131820))
        advanceUntilIdle()
        assertEquals(LocalStatus.NotCheckedOut, notCheckedOut.ready().status)
        assertEquals(LocalStatus.NotConfigured, notConfigured.ready().status)
    }

    @Test
    fun `an unreachable desktop is an error that retries`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.MachineInfo, BackendError.DesktopUnreachable("timeout"))
        val vm = viewModel(backend = backend)
        advanceUntilIdle()
        assertEquals(LocalCardState.Failed(BackendError.DesktopUnreachable("timeout")), vm.state.value.local)
        vm.refresh()
        advanceUntilIdle()
        assertInstanceOf(LocalCardState.Ready::class.java, vm.state.value.local)
    }

    @Test
    fun `a revoked device reads as not paired`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.LocalStatus, BackendError.DeviceRevoked)
        val vm = viewModel(backend = backend)
        advanceUntilIdle()
        assertEquals(LocalCardState.NotPaired, vm.state.value.local)
    }

    @Test
    fun `stashing starts from the settings default`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.setAutostash(true)
        val vm = viewModel(backend = backend)
        advanceUntilIdle()
        assertTrue(vm.state.value.autostash)
        vm.setAutostash(false)
        assertFalse(vm.state.value.autostash)
    }

    @Test
    fun `a dirty worktree refuses without stashing and updates with it`() = runTest(main.dispatcher) {
        val vm = viewModel(authorFilter)
        val messages = messagesOf(vm)
        advanceUntilIdle()
        vm.runOp(LocalOp.RebaseBase)
        assertEquals(LocalOp.RebaseBase, vm.state.value.runningOp)
        advanceUntilIdle()
        assertNull(vm.state.value.runningOp)
        assertEquals("worktree has uncommitted changes", messages.last())
        vm.setAutostash(true)
        vm.runOp(LocalOp.RebaseBase)
        advanceUntilIdle()
        assertEquals("Updated feat/author-filter", messages.last())
        assertNull((vm.ready().status as LocalStatus.CheckedOut).branch.blocker)
    }

    @Test
    fun `aborting clears the stopped rebase`() = runTest(main.dispatcher) {
        val vm = viewModel()
        val messages = messagesOf(vm)
        advanceUntilIdle()
        vm.abort()
        assertTrue(vm.state.value.aborting)
        advanceUntilIdle()
        assertFalse(vm.state.value.aborting)
        assertNull((vm.ready().status as LocalStatus.CheckedOut).branch.inProgress)
        assertEquals("Aborted the rebase", messages.single())
    }

    @Test
    fun `a failed abort says why and keeps the state`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = viewModel(backend = backend)
        val messages = messagesOf(vm)
        advanceUntilIdle()
        backend.failNext(FakeCall.AbortLocal, BackendError.DesktopTimeout)
        vm.abort()
        advanceUntilIdle()
        assertNotNull((vm.ready().status as LocalStatus.CheckedOut).branch.inProgress)
        assertEquals(1, messages.size)
    }
}
