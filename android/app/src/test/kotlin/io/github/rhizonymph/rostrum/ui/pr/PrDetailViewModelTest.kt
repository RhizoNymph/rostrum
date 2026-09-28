package io.github.rhizonymph.rostrum.ui.pr

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
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
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class PrDetailViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val diffOverview = PrRef("RhizoNymph/rostrum", 10)
    private val docsDraft = PrRef("RhizoNymph/rostrum", 11)

    private fun viewModel(backend: FakeRostrumBackend = testBackend(), pr: PrRef = diffOverview) =
        PrDetailViewModel(pr, backend, TEST_CLOCK)

    private val PrDetailViewModel.detail: PullDetail
        get() = (state.value.detail as UiState.Loaded).data

    private fun TestScope.messagesOf(vm: PrDetailViewModel): MutableList<String> {
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.toList(messages) }
        return messages
    }

    @Nested
    inner class Loading {
        @Test
        fun `starts loading, then shows the detail`() = runTest(main.dispatcher) {
            val vm = viewModel()
            assertEquals(UiState.Loading, vm.state.value.detail)
            advanceUntilIdle()
            assertEquals(10, vm.detail.header.number)
            assertEquals(TEST_NOW, vm.state.value.loadedAt)
            assertFalse(vm.state.value.refreshing)
        }

        @Test
        fun `a failed first load is an error`() = runTest(main.dispatcher) {
            val backend = testBackend()
            backend.failNext(FakeCall.PullDetail, BackendError.Network("offline"))
            val vm = viewModel(backend)
            advanceUntilIdle()
            assertEquals(UiState.Error(BackendError.Network("offline")), vm.state.value.detail)
        }

        @Test
        fun `refreshing after an error recovers`() = runTest(main.dispatcher) {
            val backend = testBackend()
            backend.failNext(FakeCall.PullDetail, BackendError.Network("offline"))
            val vm = viewModel(backend)
            advanceUntilIdle()
            vm.refresh()
            advanceUntilIdle()
            assertInstanceOf(UiState.Loaded::class.java, vm.state.value.detail)
        }

        @Test
        fun `a failed refresh keeps what is shown and says why`() = runTest(main.dispatcher) {
            val backend = testBackend()
            val vm = viewModel(backend)
            val messages = messagesOf(vm)
            advanceUntilIdle()
            backend.failNext(FakeCall.PullDetail, BackendError.Network("offline"))
            vm.refresh()
            advanceUntilIdle()
            assertInstanceOf(UiState.Loaded::class.java, vm.state.value.detail)
            assertTrue(messages.single().contains("offline"))
        }

        @Test
        fun `an unknown pull request is an error`() = runTest(main.dispatcher) {
            val vm = viewModel(pr = PrRef("nobody/nothing", 1))
            advanceUntilIdle()
            assertInstanceOf(BackendError.UnknownPullRequest::class.java, (vm.state.value.detail as UiState.Error).error)
        }
    }

    @Nested
    inner class Comments {
        @Test
        fun `posting a comment clears the composer and shows it`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            vm.setComment("Looks good to me")
            vm.postComment()
            assertTrue(vm.state.value.postingComment)
            advanceUntilIdle()
            assertEquals("", vm.state.value.comment)
            assertFalse(vm.state.value.postingComment)
            val last = vm.detail.timeline.last().kind as TimelineKind.Comment
            assertEquals("Looks good to me", last.source)
        }

        @Test
        fun `a blank comment is not posted`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            val before = vm.detail.timeline.size
            vm.setComment("   ")
            vm.postComment()
            advanceUntilIdle()
            assertEquals(before, vm.detail.timeline.size)
        }

        @Test
        fun `a failed comment keeps the text`() = runTest(main.dispatcher) {
            val backend = testBackend()
            val vm = viewModel(backend)
            val messages = messagesOf(vm)
            advanceUntilIdle()
            backend.failNext(FakeCall.AddComment, BackendError.Network("offline"))
            vm.setComment("Keep me")
            vm.postComment()
            advanceUntilIdle()
            assertEquals("Keep me", vm.state.value.comment)
            assertEquals(1, messages.size)
        }

        @Test
        fun `replying to a thread adds the reply`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            val thread = vm.detail.threads.single()
            vm.startReply(thread.id)
            vm.setReplyText("Fair, the cap handles it")
            vm.sendReply()
            advanceUntilIdle()
            assertNull(vm.state.value.reply)
            assertEquals(3, vm.detail.threads.single().comments.size)
        }

        @Test
        fun `cancelling a reply forgets it`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            vm.startReply("thread-1")
            vm.setReplyText("draft")
            vm.cancelReply()
            assertNull(vm.state.value.reply)
        }
    }

    @Nested
    inner class Labels {
        @Test
        fun `the picker loads the repository's labels`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            vm.openLabels()
            advanceUntilIdle()
            val available = (vm.state.value.labels!!.available as UiState.Loaded).data
            assertTrue(available.any { it.name == "ui" })
        }

        @Test
        fun `toggling adds and then removes a label`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            vm.openLabels()
            vm.toggleLabel("ui")
            advanceUntilIdle()
            assertTrue(vm.detail.header.labels.any { it.name == "ui" })
            assertNull(vm.state.value.labels!!.pending)
            vm.toggleLabel("ui")
            advanceUntilIdle()
            assertFalse(vm.detail.header.labels.any { it.name == "ui" })
        }

        @Test
        fun `closing the picker`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            vm.openLabels()
            vm.closeLabels()
            assertNull(vm.state.value.labels)
        }
    }

    @Nested
    inner class Actions {
        @Test
        fun `the draft toggle follows the header's action`() = runTest(main.dispatcher) {
            val vm = viewModel(pr = docsDraft)
            val messages = messagesOf(vm)
            advanceUntilIdle()
            assertTrue(vm.detail.header.isDraft)
            vm.toggleDraft()
            assertEquals(PrBusy.Draft, vm.state.value.busy)
            advanceUntilIdle()
            assertFalse(vm.detail.header.isDraft)
            assertNull(vm.state.value.busy)
            assertEquals("Marked ready for review", messages.single())
        }

        @Test
        fun `close and reopen`() = runTest(main.dispatcher) {
            val vm = viewModel()
            val messages = messagesOf(vm)
            advanceUntilIdle()
            vm.close()
            advanceUntilIdle()
            assertEquals(PullState.Closed, vm.detail.header.state)
            vm.reopen()
            advanceUntilIdle()
            assertEquals(PullState.Open, vm.detail.header.state)
            assertEquals(listOf("Closed #10", "Reopened #10"), messages)
        }

        @Test
        fun `updating the branch brings it up to date`() = runTest(main.dispatcher) {
            val vm = viewModel()
            val messages = messagesOf(vm)
            advanceUntilIdle()
            vm.updateBranch(BranchUpdateMethod.Rebase)
            assertEquals(PrBusy.UpdateRebase, vm.state.value.busy)
            advanceUntilIdle()
            assertEquals(0, vm.detail.header.divergence!!.behind)
            assertEquals("Rebased feat/diff-overview onto main", messages.single())
        }

        @Test
        fun `a refused update says why`() = runTest(main.dispatcher) {
            val backend = testBackend()
            val vm = viewModel(backend)
            val messages = messagesOf(vm)
            advanceUntilIdle()
            backend.failNext(FakeCall.UpdateBranch, BackendError.GitHubApi(422, "no new commits"))
            vm.updateBranch(BranchUpdateMethod.Merge)
            advanceUntilIdle()
            assertNull(vm.state.value.busy)
            assertTrue(messages.single().contains("no new commits"))
        }
    }

    @Nested
    inner class Merging {
        private suspend fun approved(): FakeRostrumBackend = testBackend().also {
            it.submitReview(diffOverview, ReviewEvent.Approve, "", includeDrafts = false).orFail()
        }

        @Test
        fun `the merge sheet does not open while merging is blocked`() = runTest(main.dispatcher) {
            val vm = viewModel()
            advanceUntilIdle()
            vm.openMerge()
            assertNull(vm.state.value.merge)
        }

        @Test
        fun `the sheet opens on a merge commit with GitHub's defaults, and follows the method`() = runTest(main.dispatcher) {
            val vm = viewModel(approved())
            advanceUntilIdle()
            vm.openMerge()
            val form = vm.state.value.merge!!
            assertEquals(MergeMethod.Merge, form.method)
            assertEquals("Merge pull request #10 from RhizoNymph/feat/diff-overview", form.title)
            vm.setMergeMethod(MergeMethod.Squash)
            assertEquals("feat: visual diff overview with change map and ranked churn list (#10)", vm.state.value.merge!!.title)
            assertEquals("", vm.state.value.merge!!.message)
            vm.setMergeMethod(MergeMethod.Rebase)
            assertFalse(vm.state.value.merge!!.hasCommitText)
        }

        @Test
        fun `edits to the commit text are kept`() = runTest(main.dispatcher) {
            val vm = viewModel(approved())
            advanceUntilIdle()
            vm.openMerge()
            vm.setMergeTitle("Custom title")
            vm.setMergeMessage("Body")
            assertEquals("Custom title", vm.state.value.merge!!.title)
            assertEquals("Body", vm.state.value.merge!!.message)
        }

        @Test
        fun `confirming merges, closes the sheet and refreshes`() = runTest(main.dispatcher) {
            val vm = viewModel(approved())
            val messages = messagesOf(vm)
            advanceUntilIdle()
            vm.openMerge()
            vm.setMergeMethod(MergeMethod.Squash)
            vm.confirmMerge()
            assertTrue(vm.state.value.merge!!.submitting)
            advanceUntilIdle()
            assertNull(vm.state.value.merge)
            assertEquals(PullState.Merged, vm.detail.header.state)
            assertEquals("Merged #10", messages.single())
        }

        @Test
        fun `a refused merge stays open with the reason`() = runTest(main.dispatcher) {
            val backend = approved()
            val vm = viewModel(backend)
            advanceUntilIdle()
            vm.openMerge()
            backend.failNext(FakeCall.Merge, BackendError.MergeBlocked("Required status check is failing"))
            vm.confirmMerge()
            advanceUntilIdle()
            val form = vm.state.value.merge!!
            assertFalse(form.submitting)
            assertEquals(BackendError.MergeBlocked("Required status check is failing"), form.error)
        }

        @Test
        fun `a head that moved after the sheet opened is refused`() = runTest(main.dispatcher) {
            val backend = approved()
            val vm = viewModel(backend)
            advanceUntilIdle()
            vm.openMerge()
            val head = backend.pullHeader(diffOverview).orFail().headSha
            backend.updateBranch(diffOverview, BranchUpdateMethod.Merge, head).orFail()
            vm.confirmMerge()
            advanceUntilIdle()
            assertInstanceOf(BackendError.MergeBlocked::class.java, vm.state.value.merge!!.error)
            assertEquals(PullState.Open, backend.pullHeader(diffOverview).orFail().state)
            assertNotNull(vm.state.value.merge)
        }

        @Test
        fun `dismissing forgets the form`() = runTest(main.dispatcher) {
            val vm = viewModel(approved())
            advanceUntilIdle()
            vm.openMerge()
            vm.dismissMerge()
            assertNull(vm.state.value.merge)
        }
    }
}
