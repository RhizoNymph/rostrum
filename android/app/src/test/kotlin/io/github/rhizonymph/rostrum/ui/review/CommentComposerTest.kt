package io.github.rhizonymph.rostrum.ui.review

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class CommentComposerTest {
    private val pr = PrRef("RhizoNymph/rostrum", 10)
    private val path = "crates/rostrum-diff/src/overview.rs"
    private val range = CommentTarget(CommentAnchor(path, 56, Side.Right), CommentAnchor(path, 53, Side.Right))

    private class Harness(scope: TestScope, val backend: FakeRostrumBackend, pr: PrRef) {
        var pending: PendingReview? = null
        var posted = 0
        var closed = 0
        val composer = CommentComposer(
            backend = backend,
            pr = pr,
            scope = scope,
            messages = Messages(),
            onPendingChanged = { pending = it },
            onPosted = { posted++ },
            onClosed = { closed++ },
        )
    }

    private fun TestScope.harness(backend: FakeRostrumBackend = testBackend()) = Harness(this, backend, pr)

    @Test
    fun `a new comment starts empty in Write, counting the other drafts`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        val state = h.composer.state.value!!
        assertEquals("", state.text)
        assertEquals(EditorTab.Write, state.tab)
        assertEquals(2, state.otherPending)
        assertEquals(3, state.badge)
        assertEquals("overview.rs · L53–56 · new side", state.chip)
        assertEquals("Anchored to new-file lines 53–56 (RIGHT). Removed lines anchor LEFT.", state.note)
        assertFalse(state.canSend)
        assertNotNull(state.commentNowNote)
    }

    @Test
    fun `preview renders the text as markdown`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("Could `churn` be a method?\n\n- yes")
        h.composer.setTab(EditorTab.Preview)
        val state = h.composer.state.value!!
        assertEquals(EditorTab.Preview, state.tab)
        assertEquals(MdBlockKind.Paragraph, state.preview[0].kind)
        assertTrue(state.preview[0].spans.any { it.code })
        h.composer.setTab(EditorTab.Write)
        assertEquals(EditorTab.Write, h.composer.state.value!!.tab)
    }

    @Test
    fun `adding to the review saves a draft and closes`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("Methods on DiffFile?")
        h.composer.addToReview()
        advanceUntilIdle()
        assertNull(h.composer.state.value)
        assertEquals(1, h.closed)
        val draft = h.pending!!.drafts.last()
        assertEquals(53, draft.anchor.startLine)
        assertEquals("Methods on DiffFile?", draft.body)
    }

    @Test
    fun `a failed add keeps the sheet open with the error`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("x")
        h.backend.failNext(FakeCall.AddDraft, BackendError.Network("offline"))
        h.composer.addToReview()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(BackendError.Network("offline")), h.composer.state.value!!.action)
        assertEquals("x", h.composer.state.value!!.text)
    }

    @Test
    fun `blank text is never sent`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("   ")
        h.composer.addToReview()
        advanceUntilIdle()
        assertEquals(2, h.backend.pendingReview(pr).orFail().drafts.size)
        assertNotNull(h.composer.state.value)
    }

    @Test
    fun `comment now is off while other drafts are pending`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("Methods on DiffFile?")
        val state = h.composer.state.value!!
        assertTrue(state.canSend)
        assertFalse(state.canCommentNow)
        h.composer.commentNow()
        advanceUntilIdle()
        assertEquals(0, h.posted)
        assertEquals(2, h.backend.pendingReview(pr).orFail().drafts.size)
        assertNotNull(h.composer.state.value)
    }

    @Test
    fun `comment now posts a lone comment at once`() = runTest {
        val h = harness()
        h.backend.discardDrafts(pr).orFail()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("Methods on DiffFile?")
        assertTrue(h.composer.state.value!!.canCommentNow)
        assertNull(h.composer.state.value!!.commentNowNote)
        h.composer.commentNow()
        advanceUntilIdle()
        assertNull(h.composer.state.value)
        assertEquals(1, h.posted)
        assertTrue(h.pending!!.drafts.isEmpty())
        assertEquals(2, h.backend.pullDetail(pr).orFail().threads.size)
    }

    @Test
    fun `when sending fails after saving, the draft stays pending`() = runTest {
        val h = harness()
        h.backend.discardDrafts(pr).orFail()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("x")
        h.backend.failNext(FakeCall.SubmitReview, BackendError.Network("offline"))
        h.composer.commentNow()
        advanceUntilIdle()
        assertNull(h.composer.state.value)
        assertEquals(0, h.posted)
        assertEquals(1, h.pending!!.drafts.size)
    }

    @Test
    fun `stale drafts block new comments`() = runTest {
        val backend = testBackend()
        val stalePr = PrRef("RhizoNymph/rostrum", 9)
        val h = Harness(this, backend, stalePr)
        val pending = backend.pendingReview(stalePr).orFail()
        val anchor = CommentAnchor("src/settings.rs", 20, Side.Right)
        h.composer.openNew(CommentTarget(anchor), pending)
        h.composer.setText("x")
        val state = h.composer.state.value!!
        assertTrue(state.stale)
        assertNotNull(state.blockedReason)
        assertFalse(state.canSend)
    }

    @Test
    fun `editing loads the body, saves and deletes`() = runTest {
        val h = harness()
        val pending = h.backend.pendingReview(pr).orFail()
        val draft = pending.drafts.first()
        h.composer.openEdit(draft, pending)
        val state = h.composer.state.value!!
        assertEquals(ComposerMode.Edit(draft), state.mode)
        assertEquals(draft.body, state.text)
        assertEquals(1, state.otherPending)
        assertNull(state.commentNowNote)
        h.composer.setText("Edited")
        h.composer.save()
        advanceUntilIdle()
        assertEquals("Edited", h.pending!!.drafts.first().body)

        h.composer.openEdit(h.pending!!.drafts.first(), h.pending!!)
        h.composer.delete()
        advanceUntilIdle()
        assertEquals(1, h.pending!!.drafts.size)
        assertNull(h.composer.state.value)
    }

    @Test
    fun `dismiss closes without saving`() = runTest {
        val h = harness()
        h.composer.openNew(range, h.backend.pendingReview(pr).orFail())
        h.composer.setText("draft text")
        h.composer.dismiss()
        assertNull(h.composer.state.value)
        assertEquals(1, h.closed)
        assertEquals(2, h.backend.pendingReview(pr).orFail().drafts.size)
    }
}
