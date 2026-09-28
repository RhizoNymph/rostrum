package io.github.rhizonymph.rostrum.ui.pr.files

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.review.ComposerMode
import kotlinx.coroutines.test.TestScope
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

class FileDiffViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val pr = PrRef("RhizoNymph/rostrum", 10)

    /** overview.rs in crates/rostrum-diff: patch index 1, the largest file. */
    private val overviewRs = 1

    private fun TestScope.open(fileIndex: Int = overviewRs, target: PrRef = pr): FileDiffViewModel {
        val vm = FileDiffViewModel(testBackend(), target, fileIndex)
        advanceUntilIdle()
        return vm
    }

    private fun FileDiffViewModel.rowOfNew(line: Int) =
        state.value.rows.indexOfFirst { it is DiffRow.Line && it.line.newLine == line && it.line.kind != LineKind.Removed }

    @Nested
    inner class Loading {
        @Test
        fun `loads the file, its place in the ranked order, and the pending review`() = runTest(main.dispatcher) {
            val vm = open()
            val state = vm.state.value
            assertEquals("crates/rostrum-diff/src/overview.rs", state.file!!.path)
            assertEquals("1/7", state.nav!!.label)
            assertFalse(state.nav!!.hasPrevious)
            assertTrue(state.nav!!.hasNext)
            assertInstanceOf(UiState.Loaded::class.java, state.diff)
            assertEquals(2, state.pending!!.drafts.size)
        }

        @Test
        fun `the smallest file is last`() = runTest(main.dispatcher) {
            val nav = open(fileIndex = 0).state.value.nav!!
            assertEquals("7/7", nav.label)
            assertFalse(nav.hasNext)
        }

        @Test
        fun `a failed diff shows the error and retry recovers`() = runTest(main.dispatcher) {
            val backend = testBackend()
            backend.failNext(FakeCall.FileDiff, BackendError.Network("offline"))
            val vm = FileDiffViewModel(backend, pr, overviewRs)
            advanceUntilIdle()
            assertEquals(UiState.Error(BackendError.Network("offline")), vm.state.value.diff)
            vm.retry()
            advanceUntilIdle()
            assertInstanceOf(UiState.Loaded::class.java, vm.state.value.diff)
        }

        @Test
        fun `without the overview the diff still loads, with no navigation`() = runTest(main.dispatcher) {
            val backend = testBackend()
            backend.failNext(FakeCall.FilesOverview, BackendError.Network("offline"))
            val vm = FileDiffViewModel(backend, pr, overviewRs)
            advanceUntilIdle()
            assertNull(vm.state.value.nav)
            assertInstanceOf(UiState.Loaded::class.java, vm.state.value.diff)
            assertEquals("crates/rostrum-diff/src/overview.rs", vm.state.value.file!!.path)
        }
    }

    @Nested
    inner class Navigation {
        @Test
        fun `next and previous follow the ranked order`() = runTest(main.dispatcher) {
            val vm = open()
            vm.nextFile()
            advanceUntilIdle()
            assertEquals(4, vm.state.value.fileIndex)
            assertEquals("2/7", vm.state.value.nav!!.label)
            assertEquals("crates/rostrum/src/detail/overview.rs", vm.state.value.file!!.path)
            vm.previousFile()
            advanceUntilIdle()
            assertEquals(overviewRs, vm.state.value.fileIndex)
        }

        @Test
        fun `previous at the start does nothing`() = runTest(main.dispatcher) {
            val vm = open()
            vm.previousFile()
            advanceUntilIdle()
            assertEquals(overviewRs, vm.state.value.fileIndex)
        }

        @Test
        fun `soft wrap is a toggle, viewed is remembered per file`() = runTest(main.dispatcher) {
            val vm = open()
            vm.toggleSoftWrap()
            assertTrue(vm.state.value.softWrap)
            vm.toggleViewed()
            assertTrue(vm.state.value.isViewed)
            vm.nextFile()
            advanceUntilIdle()
            assertFalse(vm.state.value.isViewed)
            assertTrue(vm.state.value.softWrap)
            vm.previousFile()
            advanceUntilIdle()
            assertTrue(vm.state.value.isViewed)
        }
    }

    @Nested
    inner class Comments {
        @Test
        fun `tapping a line number opens the composer on that line`() = runTest(main.dispatcher) {
            val vm = open()
            val row = vm.rowOfNew(60)
            vm.onLineNumberTap(row)
            val composer = vm.composer.state.value!!
            val mode = composer.mode as ComposerMode.New
            assertEquals(60, mode.target.line)
            assertNull(mode.target.rangeStart)
            assertEquals(setOf(row), vm.state.value.selectedRows)
        }

        @Test
        fun `tapping a hunk header does nothing`() = runTest(main.dispatcher) {
            val vm = open()
            vm.onLineNumberTap(0)
            assertNull(vm.composer.state.value)
        }

        @Test
        fun `dragging selects a range and releasing opens the composer`() = runTest(main.dispatcher) {
            val vm = open()
            vm.onSelectionStart(vm.rowOfNew(53))
            vm.onSelectionMove(vm.rowOfNew(56))
            assertEquals(4, vm.state.value.selectedRows.size)
            vm.onSelectionEnd()
            val target = (vm.composer.state.value!!.mode as ComposerMode.New).target
            assertEquals(53, target.startLine)
            assertEquals(56, target.line)
        }

        @Test
        fun `adding to the review shows the draft in the diff and clears the selection`() = runTest(main.dispatcher) {
            val vm = open()
            vm.onSelectionStart(vm.rowOfNew(53))
            vm.onSelectionMove(vm.rowOfNew(56))
            vm.onSelectionEnd()
            vm.composer.setText("Could these be methods on DiffFile?")
            vm.composer.addToReview()
            advanceUntilIdle()
            assertNull(vm.composer.state.value)
            assertTrue(vm.state.value.selectedRows.isEmpty())
            assertEquals(3, vm.state.value.pending!!.drafts.size)
            val rows = vm.state.value.rows
            val after56 = rows[vm.rowOfNew(56) + 1]
            assertInstanceOf(DiffRow.Draft::class.java, after56)
        }

        @Test
        fun `dismissing the composer clears the selection`() = runTest(main.dispatcher) {
            val vm = open()
            vm.onLineNumberTap(vm.rowOfNew(10))
            vm.composer.dismiss()
            assertNull(vm.composer.state.value)
            assertTrue(vm.state.value.selectedRows.isEmpty())
        }

        @Test
        fun `deleting a draft removes it from the diff`() = runTest(main.dispatcher) {
            val vm = open()
            val draft = vm.state.value.rows.filterIsInstance<DiffRow.Draft>().single().draft
            vm.deleteDraft(draft.id)
            advanceUntilIdle()
            assertEquals(1, vm.state.value.pending!!.drafts.size)
            assertTrue(vm.state.value.rows.none { it is DiffRow.Draft })
        }

        @Test
        fun `editing a draft opens it and saving updates the diff`() = runTest(main.dispatcher) {
            val vm = open()
            val draft = vm.state.value.rows.filterIsInstance<DiffRow.Draft>().single().draft
            vm.editDraft(draft.id)
            val composer = vm.composer.state.value!!
            assertEquals(ComposerMode.Edit(draft), composer.mode)
            assertEquals(draft.body, composer.text)
            vm.composer.setText("Edited")
            vm.composer.save()
            advanceUntilIdle()
            assertEquals("Edited", vm.state.value.rows.filterIsInstance<DiffRow.Draft>().single().draft.body)
        }

        @Test
        fun `a stale pending review blocks new comments`() = runTest(main.dispatcher) {
            val vm = open(fileIndex = 0, target = PrRef("RhizoNymph/rostrum", 9))
            assertTrue(vm.state.value.pending!!.stale)
            val row = vm.state.value.rows.indexOfFirst { it is DiffRow.Line }
            vm.onLineNumberTap(row)
            val composer = vm.composer.state.value!!
            assertNotNull(composer.blockedReason)
            vm.composer.setText("x")
            assertFalse(composer.copy(text = "x").canSend)
        }
    }

    @Nested
    inner class Replies {
        @Test
        fun `replying adds the comment to the thread`() = runTest(main.dispatcher) {
            val vm = open()
            vm.startReply("thread-1")
            vm.setReplyText("Fair, a cap it is.")
            vm.sendReply()
            advanceUntilIdle()
            assertNull(vm.state.value.reply)
            val thread = vm.state.value.rows.filterIsInstance<DiffRow.Thread>().single().thread
            assertEquals(3, thread.comments.size)
        }

        @Test
        fun `an empty reply is not sent`() = runTest(main.dispatcher) {
            val vm = open()
            vm.startReply("thread-1")
            vm.sendReply()
            advanceUntilIdle()
            assertEquals(ActionState.Idle, vm.state.value.reply!!.action)
        }

        @Test
        fun `a failed reply keeps the text`() = runTest(main.dispatcher) {
            val backend = testBackend()
            val vm = FileDiffViewModel(backend, pr, overviewRs)
            advanceUntilIdle()
            vm.startReply("thread-1")
            vm.setReplyText("hello")
            backend.failNext(FakeCall.ReplyToThread, BackendError.Network("offline"))
            vm.sendReply()
            advanceUntilIdle()
            val reply = vm.state.value.reply!!
            assertEquals("hello", reply.text)
            assertEquals(ActionState.Failed(BackendError.Network("offline")), reply.action)
        }
    }

    @Test
    fun `a submitted review reloads the diff and the pending review`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = FileDiffViewModel(backend, pr, overviewRs)
        advanceUntilIdle()
        backend.submitReview(pr, io.github.rhizonymph.rostrum.data.model.ReviewEvent.Comment, "", includeDrafts = true)
        vm.onReviewSubmitted()
        advanceUntilIdle()
        assertTrue(vm.state.value.pending!!.drafts.isEmpty())
        assertEquals(2, vm.state.value.rows.count { it is DiffRow.Thread })
    }
}
