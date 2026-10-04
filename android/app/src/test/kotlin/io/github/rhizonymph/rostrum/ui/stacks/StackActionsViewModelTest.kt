package io.github.rhizonymph.rostrum.ui.stacks

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.StackJob
import io.github.rhizonymph.rostrum.data.model.StackJobKind
import io.github.rhizonymph.rostrum.data.model.StackJobResult
import io.github.rhizonymph.rostrum.data.model.StackJobState
import io.github.rhizonymph.rostrum.data.model.StackKind
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanCheck
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.data.model.StackRewrite
import io.github.rhizonymph.rostrum.data.model.StackSummary
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.items.StackMenuEntry
import io.github.rhizonymph.rostrum.ui.items.menuEntries
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

@OptIn(ExperimentalCoroutinesApi::class)
class StackActionsViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val rostrum = "RhizoNymph/rostrum"
    private val backend = testBackend()
    private val paired = SessionState.Ready(GitHubAuth.SignedIn, DesktopLink.Paired(RemoteStatus.Paired(listOf("h"), 8485, "4F2A", "h")))
    private val session = MutableStateFlow<SessionState>(paired)

    private class Harness(val vm: StackActionsViewModel, val messages: List<String>, val stack: PullItem.Stack, val open: List<PrSummary>)

    private suspend fun TestScope.harness(): Harness {
        val vm = StackActionsViewModel(backend, session, pollMillis = 10)
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.collect { messages += it } }
        val overview = backend.repoOverview(rostrum).orFail()
        val stack = overview.pulls.filterIsInstance<PullItem.Stack>().single()
        return Harness(vm, messages, stack, overview.pulls.flatMap { it.pulls })
    }

    private fun Harness.request(entry: StackMenuEntry) = vm.request(entry, rostrum, stack.stack, stack.members)

    @Test
    fun `menus offer what each kind of stack allows`() {
        val github = StackSummary(StackKind.GitHub(7), "Stack 7 · 2 PRs", "main", 2, 0, null)
        assertEquals(listOf(StackMenuEntry.Merge, StackMenuEntry.AddTo, StackMenuEntry.Unstack), menuEntries(github))
        assertEquals(listOf(StackMenuEntry.Make), menuEntries(github.copy(kind = StackKind.Chain)))
        assertEquals(7, github.number)
    }

    @Test
    fun `without a desktop every action says so`() = runTest(main.dispatcher) {
        session.value = SessionState.Ready(GitHubAuth.SignedIn, DesktopLink.NotPaired)
        val h = harness()
        h.request(StackMenuEntry.Merge)
        assertEquals(StackFlow.NeedsDesktop("Merge stack"), h.vm.flow.value)
        h.vm.dismiss()
        h.vm.startArrange(rostrum)
        assertEquals(StackFlow.NeedsDesktop("Arrange"), h.vm.flow.value)
        assertNull(h.vm.selection.value)
    }

    @Test
    fun `merge confirms the members and method, then follows the job to the end`() = runTest(main.dispatcher) {
        val h = harness()
        h.request(StackMenuEntry.Merge)
        val confirm = h.vm.flow.value as StackFlow.ConfirmMerge
        assertEquals(listOf(9, 11), confirm.members.map { it.number })
        h.vm.setMergeMethod(StackMergeMethod.Squash)
        assertEquals(StackMergeMethod.Squash, (h.vm.flow.value as StackFlow.ConfirmMerge).method)
        h.vm.confirm()
        advanceUntilIdle()
        val job = (h.vm.flow.value as StackFlow.Job).job
        assertTrue(job.finished)
        assertEquals(StackJobResult.Merged(7), (job.state as StackJobState.Done).result)
        assertEquals(listOf("Merged stack 7"), h.messages)
        h.vm.dismiss()
        assertNull(h.vm.flow.value)
    }

    @Test
    fun `unstack asks first`() = runTest(main.dispatcher) {
        val h = harness()
        h.request(StackMenuEntry.Unstack)
        assertInstanceOf(StackFlow.ConfirmUnstack::class.java, h.vm.flow.value)
        h.vm.confirm()
        advanceUntilIdle()
        assertEquals("Unstacked stack 7", h.messages.single())
    }

    @Test
    fun `adding lists candidates, plans, shows the branches, and confirms exactly those`() = runTest(main.dispatcher) {
        val h = harness()
        h.request(StackMenuEntry.AddTo)
        advanceUntilIdle()
        val pick = h.vm.flow.value as StackFlow.PickExtend
        assertEquals(listOf(10), (pick.candidates as UiState.Loaded).data.map { it.number })
        h.vm.toggleCandidate(10)
        h.vm.planExtend()
        advanceUntilIdle()
        val rewrite = h.vm.flow.value as StackFlow.ConfirmRewrite
        assertEquals(StackPlanRequest.Extend(rostrum, 7, listOf(10)), rewrite.request)
        assertEquals(listOf(StackRewrite(10, "feat/diff-overview")), rewrite.rewrites)
        h.vm.confirm()
        advanceUntilIdle()
        val job = (h.vm.flow.value as StackFlow.Job).job
        assertEquals(StackJobKind.Extend, job.kind)
        assertEquals(StackJobResult.Extended(7, listOf(10)), (job.state as StackJobState.Done).result)
    }

    @Test
    fun `a refused rewrite shows the desktop's branches and asks again`() = runTest(main.dispatcher) {
        val h = harness()
        h.request(StackMenuEntry.AddTo)
        advanceUntilIdle()
        h.vm.toggleCandidate(10)
        h.vm.planExtend()
        advanceUntilIdle()
        val changed = listOf(StackRewrite(10, "feat/diff-overview"), StackRewrite(11, "docs/android-design"))
        backend.failNext(FakeCall.ExtendStack, BackendError.RewriteNotConfirmed(changed, "the stack moved"))
        h.vm.confirm()
        advanceUntilIdle()
        val again = h.vm.flow.value as StackFlow.ConfirmRewrite
        assertEquals(changed, again.rewrites)
        assertEquals("the stack moved", again.note)
        assertEquals(ActionState.Idle, again.run)
    }

    @Test
    fun `a busy desktop says so and keeps the question open`() = runTest(main.dispatcher) {
        val h = harness()
        h.request(StackMenuEntry.Unstack)
        backend.failNext(FakeCall.Unstack, BackendError.RemoteApi(RemoteErrorCode.Busy, "busy"))
        h.vm.confirm()
        advanceUntilIdle()
        assertInstanceOf(StackFlow.ConfirmUnstack::class.java, h.vm.flow.value)
        assertEquals("The desktop is busy with another job.", h.messages.single())
    }

    @Test
    fun `make stack on a detected chain runs with no rewrite`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.request(StackMenuEntry.Make, rostrum, h.stack.stack.copy(kind = StackKind.Chain), h.stack.members)
        val make = h.vm.flow.value as StackFlow.ConfirmMake
        assertEquals("main", make.trunk)
        h.vm.confirm()
        advanceUntilIdle()
        assertEquals(StackJobKind.Make, (h.vm.flow.value as StackFlow.Job).job.kind)
    }

    @Test
    fun `arrange picks, orders, checks, plans and confirms`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.startArrange(rostrum)
        h.open.first { it.number == 11 }.let(h.vm::toggleSelected)
        h.open.first { it.number == 9 }.let(h.vm::toggleSelected)
        assertEquals(listOf(11, 9), h.vm.selection.value!!.picked)
        h.vm.arrangeNext(h.open)
        advanceUntilIdle()
        assertNull(h.vm.selection.value)
        val order = h.vm.flow.value as StackFlow.OrderArrange
        assertEquals(listOf(11, 9), order.members.map { it.number })
        assertEquals("feat/author-filter", order.trunk)
        h.vm.setTrunk("main")
        h.vm.moveDown(0)
        advanceUntilIdle()
        val reordered = h.vm.flow.value as StackFlow.OrderArrange
        assertEquals(listOf(9, 11), reordered.members.map { it.number })
        assertEquals(StackPlanCheck.Valid(emptyList()), reordered.check)
        h.vm.planArrange()
        advanceUntilIdle()
        val rewrite = h.vm.flow.value as StackFlow.ConfirmRewrite
        assertTrue(rewrite.rewrites.isEmpty())
        h.vm.confirm()
        advanceUntilIdle()
        assertEquals(StackJobKind.Arrange, (h.vm.flow.value as StackFlow.Job).job.kind)
    }

    @Test
    fun `arranging needs two pull requests`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.startArrange(rostrum)
        h.vm.toggleSelected(h.open.first())
        h.vm.arrangeNext(h.open)
        assertEquals("Pick at least two pull requests to arrange", h.messages.single())
        h.vm.cancelArrange()
        assertNull(h.vm.selection.value)
    }

    @Test
    fun `outcomes read as what happened, with the attach command for a handed-off conflict`() {
        fun job(state: StackJobState) = StackJob(1, rostrum, StackJobKind.Arrange, TEST_NOW, TEST_NOW, true, state)
        assertEquals("Stacked; rewrote #10, #11", jobOutcome(job(StackJobState.Done(StackJobResult.Stacked(listOf(10, 11), true), ""))))
        assertEquals("Conflicts in #11 went to tmux session rostrum-11", jobOutcome(job(StackJobState.HandedOff(11, "rostrum-11", "/w", ""))))
        assertEquals("Failed after pushing #9", jobOutcome(job(StackJobState.Failed(listOf(9), ""))))
        assertEquals("Failed; nothing was pushed", jobOutcome(job(StackJobState.Failed(emptyList(), ""))))
        assertEquals("Stopped on conflicts in #11", jobOutcome(job(StackJobState.Conflicted(11, ""))))
        assertEquals("tmux attach -t rostrum-11", attachCommand("rostrum-11"))
        assertEquals("Arranging the stack", jobTitle(StackJobKind.Arrange))
    }

    @Test
    fun `a handed-off job is reported with its session`() = runTest(main.dispatcher) {
        val h = harness()
        backend.nextStackOutcome = StackJobState.HandedOff(11, "rostrum-11", "/home/w", "conflict in src/lib.rs")
        h.request(StackMenuEntry.Merge)
        h.vm.confirm()
        advanceUntilIdle()
        val state = (h.vm.flow.value as StackFlow.Job).job.state as StackJobState.HandedOff
        assertEquals("rostrum-11", state.session)
        assertEquals("Conflicts in #11 went to tmux session rostrum-11", h.messages.single())
    }
}
