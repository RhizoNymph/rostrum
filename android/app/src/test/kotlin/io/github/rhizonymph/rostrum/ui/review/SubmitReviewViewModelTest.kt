package io.github.rhizonymph.rostrum.ui.review

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
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

class SubmitReviewViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val diffOverview = PrRef("RhizoNymph/rostrum", 10)
    private val yoursAndStale = PrRef("RhizoNymph/rostrum", 9)

    private fun TestScope.open(pr: PrRef = diffOverview, backend: io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend = testBackend()): SubmitReviewViewModel {
        val vm = SubmitReviewViewModel(backend, pr)
        vm.load()
        advanceUntilIdle()
        return vm
    }

    private fun SubmitReviewViewModel.content() = (state.value.content as UiState.Loaded).data

    @Test
    fun `loads the header and the pending drafts, defaulting to Comment`() = runTest(main.dispatcher) {
        val vm = open()
        val content = vm.content()
        assertEquals(10, content.header.number)
        assertEquals(2, content.pending.drafts.size)
        assertEquals(ReviewEvent.Comment, vm.state.value.verdict)
        ReviewEvent.entries.forEach { assertNull(vm.state.value.blockedReason(it)) }
        assertTrue(vm.state.value.canSubmit)
    }

    @Test
    fun `approving submits and unblocks the merge`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = open(backend = backend)
        var submitted = 0
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.submitted.collect { submitted++ } }
        vm.setVerdict(ReviewEvent.Approve)
        vm.setSummary("Clean split. Two nits inline.")
        vm.submit()
        advanceUntilIdle()
        assertEquals(ActionState.Idle, vm.state.value.submit)
        assertEquals(1, submitted)
        assertEquals(MergeStatus.Ready, backend.pullHeader(diffOverview).orFail().merge.status)
        assertTrue(backend.pendingReview(diffOverview).orFail().drafts.isEmpty())
        assertEquals("", vm.state.value.summary)
        assertEquals(ReviewEvent.Comment, vm.state.value.verdict)
    }

    @Test
    fun `a comment review sends the drafts even with no summary`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = open(backend = backend)
        vm.submit()
        advanceUntilIdle()
        assertEquals(3, backend.pullDetail(diffOverview).orFail().threads.size)
    }

    @Test
    fun `stale drafts and your own pull request block approving and requesting changes`() = runTest(main.dispatcher) {
        val vm = open(yoursAndStale)
        val state = vm.state.value
        assertTrue(vm.content().pending.stale)
        assertNull(state.blockedReason(ReviewEvent.Comment))
        assertNotNull(state.blockedReason(ReviewEvent.Approve))
        assertNotNull(state.blockedReason(ReviewEvent.RequestChanges))
        assertNotNull(state.staleWarning)
        vm.setVerdict(ReviewEvent.Approve)
        assertEquals(ReviewEvent.Comment, vm.state.value.verdict)
    }

    @Test
    fun `a stale review can still be sent as a comment, without its drafts`() = runTest(main.dispatcher) {
        val vm = open(yoursAndStale)
        assertFalse(vm.state.value.canSubmit)
        vm.setSummary("Pushed a fix; ignore my old notes.")
        assertTrue(vm.state.value.canSubmit)
        assertFalse(vm.state.value.includesDrafts)
        vm.submit()
        advanceUntilIdle()
        assertEquals(ActionState.Idle, vm.state.value.submit)
    }

    @Test
    fun `discarding asks first, then clears the drafts and the staleness`() = runTest(main.dispatcher) {
        val vm = open(yoursAndStale)
        vm.requestDiscard()
        assertTrue(vm.state.value.confirmingDiscard)
        vm.cancelDiscard()
        assertFalse(vm.state.value.confirmingDiscard)
        vm.requestDiscard()
        vm.confirmDiscard()
        advanceUntilIdle()
        assertFalse(vm.state.value.confirmingDiscard)
        assertTrue(vm.content().pending.drafts.isEmpty())
        assertFalse(vm.content().pending.stale)
        assertNull(vm.state.value.staleWarning)
    }

    @Test
    fun `an empty comment cannot be submitted`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.discardDrafts(diffOverview)
        val vm = open(backend = backend)
        assertFalse(vm.state.value.canSubmit)
        vm.setVerdict(ReviewEvent.Approve)
        assertTrue(vm.state.value.canSubmit)
    }

    @Test
    fun `a failed submit keeps the summary and shows the error`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = open(backend = backend)
        vm.setSummary("keep me")
        backend.failNext(FakeCall.SubmitReview, BackendError.Network("offline"))
        vm.submit()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(BackendError.Network("offline")), vm.state.value.submit)
        assertEquals("keep me", vm.state.value.summary)
    }

    @Test
    fun `a failed load shows the error and retry recovers`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.PullHeader, BackendError.Network("offline"))
        val vm = open(backend = backend)
        assertInstanceOf(UiState.Error::class.java, vm.state.value.content)
        vm.load()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.content)
    }

    @Test
    fun `editing a draft from the sheet updates the list`() = runTest(main.dispatcher) {
        val vm = open()
        val draft = vm.content().pending.drafts.first()
        vm.editDraft(draft.id)
        vm.composer.setText("Rewritten")
        vm.composer.save()
        advanceUntilIdle()
        assertEquals("Rewritten", vm.content().pending.drafts.first().body)
    }
}
