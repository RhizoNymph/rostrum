package io.github.rhizonymph.rostrum.ui.issue

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/** Editing an issue's title and description, with the conflict check, and paging its timeline. */
@OptIn(ExperimentalCoroutinesApi::class)
class IssueEditTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val backend = testBackend()
    private val ref = IssueRef("RhizoNymph/rostrum", 21)

    private class Harness(val vm: IssueViewModel, val messages: List<String>)

    private fun TestScope.harness(issue: IssueRef = ref): Harness {
        val vm = IssueViewModel(backend, issue, TEST_CLOCK)
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.collect { messages += it } }
        advanceUntilIdle()
        return Harness(vm, messages)
    }

    private val Harness.description: String
        get() = (vm.state.value.detail.dataOrNull()!!.timeline.first().kind as TimelineKind.Description).source

    @Test
    fun `the editor starts from the issue as shown, with its updatedAt as the base`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Description)
        val editor = h.vm.state.value.editor!!
        assertEquals(EditField.Description, editor.field)
        assertEquals(h.vm.state.value.issue!!.updatedAt, editor.base)
        assertEquals(h.vm.state.value.issue!!.title, editor.title)
        assertEquals(h.description, editor.body)
        assertTrue(editor.canSave)
    }

    @Test
    fun `saving a new title and description updates the screen`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Title)
        h.vm.onEditTitle("Keep the scroll position")
        h.vm.onEditBody("Steps below.")
        h.vm.saveEdit()
        advanceUntilIdle()
        assertNull(h.vm.state.value.editor)
        assertEquals("Keep the scroll position", h.vm.state.value.issue!!.title)
        assertEquals("Steps below.", h.description)
        assertEquals(listOf("Saved #21"), h.messages)
    }

    @Test
    fun `a blank title can't be saved`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Title)
        h.vm.onEditTitle("  ")
        assertFalse(h.vm.state.value.editor!!.canSave)
        h.vm.saveEdit()
        advanceUntilIdle()
        assertEquals(ActionState.Idle, h.vm.state.value.editor!!.save)
    }

    @Test
    fun `preview renders the draft description`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Description)
        h.vm.onEditBody("Some **bold**")
        h.vm.showEditMode(EditMode.Preview)
        assertEquals(EditMode.Preview, h.vm.state.value.editor!!.mode)
        assertTrue(h.vm.state.value.editor!!.preview.isNotEmpty())
    }

    @Test
    fun `a conflict offers theirs, and reload takes them`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Title)
        backend.editIssueElsewhere(ref, "Their title", "Their body")
        h.vm.onEditTitle("Mine")
        h.vm.saveEdit()
        advanceUntilIdle()
        val conflict = h.vm.state.value.editor!!.conflict!!
        assertEquals("Their title", conflict.title)
        assertFalse(h.vm.state.value.editor!!.canSave)
        h.vm.reloadTheirs()
        advanceUntilIdle()
        assertNull(h.vm.state.value.editor)
        assertEquals("Their title", h.vm.state.value.issue!!.title)
    }

    @Test
    fun `a conflict can be overwritten with the draft`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Description)
        backend.editIssueElsewhere(ref, "Their title", "Their body")
        h.vm.onEditTitle("Mine")
        h.vm.onEditBody("My body")
        h.vm.saveEdit()
        advanceUntilIdle()
        h.vm.overwrite()
        advanceUntilIdle()
        assertNull(h.vm.state.value.editor)
        assertEquals("Mine", h.vm.state.value.issue!!.title)
        assertEquals("My body", h.description)
    }

    @Test
    fun `other failures stay in the editor with the error`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openEditor(EditField.Title)
        h.vm.onEditTitle("Mine")
        backend.failNext(FakeCall.EditIssue, BackendError.Network("offline"))
        h.vm.saveEdit()
        advanceUntilIdle()
        assertInstanceOf(ActionState.Failed::class.java, h.vm.state.value.editor!!.save)
        h.vm.closeEditor()
        assertNull(h.vm.state.value.editor)
    }

    @Test
    fun `load earlier brings the previous comments in after the description`() = runTest(main.dispatcher) {
        val h = harness(IssueRef("RhizoNymph/rostrum", 18))
        assertTrue(h.vm.state.value.detail.dataOrNull()!!.hasEarlier)
        h.vm.loadEarlier()
        advanceUntilIdle()
        val detail = h.vm.state.value.detail.dataOrNull()!!
        assertFalse(detail.hasEarlier)
        assertEquals("issue-18-old-1", detail.timeline[1].id)
        assertFalse(h.vm.state.value.loadingEarlier)
    }

    @Test
    fun `editor titles`() {
        assertEquals("Edit title", editorTitle(EditField.Title))
        assertEquals("Edit description", editorTitle(EditField.Description))
    }
}
