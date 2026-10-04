package io.github.rhizonymph.rostrum.ui.issue

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.IssueCloseReason
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.IssueStatus
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.PickerOption
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

@OptIn(ExperimentalCoroutinesApi::class)
class IssueViewModelTest {
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

    private val Harness.issue get() = vm.state.value.issue!!

    @Test
    fun `loads the issue with its timeline`() = runTest(main.dispatcher) {
        val h = harness()
        assertEquals(21, h.issue.number)
        assertFalse(h.vm.state.value.refreshing)
        val timeline = (h.vm.state.value.detail as UiState.Loaded).data.timeline
        assertInstanceOf(TimelineKind.Description::class.java, timeline.first().kind)
    }

    @Test
    fun `a cached issue paints first and a failed fetch keeps it`() = runTest(main.dispatcher) {
        backend.issueDetail(ref).orFail()
        backend.failNext(FakeCall.IssueDetail, BackendError.Network("offline"))
        val h = harness()
        assertEquals(21, h.issue.number)
        assertTrue(h.messages.single().contains("offline"))
    }

    @Test
    fun `an issue that can't load shows the error and retry recovers`() = runTest(main.dispatcher) {
        backend.failNext(FakeCall.IssueDetail, BackendError.Network("offline"))
        val h = harness()
        assertEquals(UiState.Error(BackendError.Network("offline")), h.vm.state.value.detail)
        h.vm.retry()
        advanceUntilIdle()
        assertEquals(21, h.issue.number)
    }

    @Test
    fun `a comment is sent, cleared, and shows in the timeline`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.sendComment()
        assertFalse(h.vm.state.value.sending)
        h.vm.onCommentChange("On it")
        h.vm.sendComment()
        advanceUntilIdle()
        assertEquals("", h.vm.state.value.comment)
        val timeline = (h.vm.state.value.detail as UiState.Loaded).data.timeline
        assertEquals("On it", (timeline.last().kind as TimelineKind.Comment).source)
    }

    @Test
    fun `a failed comment keeps the text`() = runTest(main.dispatcher) {
        val h = harness()
        backend.failNext(FakeCall.CommentOnIssue, BackendError.Network("offline"))
        h.vm.onCommentChange("On it")
        h.vm.sendComment()
        advanceUntilIdle()
        assertEquals("On it", h.vm.state.value.comment)
        assertTrue(h.messages.single().contains("offline"))
    }

    @Test
    fun `the menu closes two ways while open and reopens once closed`() = runTest(main.dispatcher) {
        val h = harness()
        assertEquals(
            listOf(IssueStateAction.Close(CloseIssueAs.Completed), IssueStateAction.Close(CloseIssueAs.NotPlanned)),
            stateActions(h.issue),
        )
        h.vm.run(IssueStateAction.Close(CloseIssueAs.NotPlanned))
        advanceUntilIdle()
        assertEquals(IssueStatus.Closed(IssueCloseReason.NotPlanned), h.issue.status)
        assertEquals(listOf("Closed #21 as not planned"), h.messages)
        assertEquals(listOf(IssueStateAction.Reopen), stateActions(h.issue))
        h.vm.run(IssueStateAction.Reopen)
        advanceUntilIdle()
        assertEquals(IssueStatus.Open, h.issue.status)
        assertEquals("Reopened #21", h.messages.last())
    }

    @Test
    fun `a failed close says why and stays open`() = runTest(main.dispatcher) {
        val h = harness()
        backend.failNext(FakeCall.CloseIssue, BackendError.GitHubApi(403, "forbidden"))
        h.vm.run(IssueStateAction.Close(CloseIssueAs.Completed))
        advanceUntilIdle()
        assertEquals(IssueStatus.Open, h.issue.status)
        assertInstanceOf(ActionState.Failed::class.java, h.vm.state.value.stateAction)
    }

    @Test
    fun `the label picker lists the repository's labels and toggles them on the issue`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openPicker(PickerKind.Labels)
        advanceUntilIdle()
        val options = (h.vm.state.value.picker!!.options as UiState.Loaded).data
        assertTrue(options.any { it is PickerOption.Label && it.key == "documentation" })
        assertEquals(setOf("bug", "ui"), h.vm.state.value.pickerSelection)
        h.vm.toggle("documentation")
        advanceUntilIdle()
        h.vm.toggle("ui")
        advanceUntilIdle()
        assertEquals(setOf("bug", "documentation"), h.vm.state.value.pickerSelection)
        assertNull(h.vm.state.value.picker!!.pending)
        h.vm.closePicker()
        assertNull(h.vm.state.value.picker)
    }

    @Test
    fun `the assignee picker assigns and unassigns`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.openPicker(PickerKind.Assignees)
        advanceUntilIdle()
        h.vm.toggle("mkowal")
        advanceUntilIdle()
        assertEquals(setOf("RhizoNymph", "mkowal"), h.issue.assignees.map { it.login }.toSet())
        h.vm.toggle("RhizoNymph")
        advanceUntilIdle()
        assertEquals(listOf("mkowal"), h.issue.assignees.map { it.login })
    }

    @Test
    fun `a picker whose options fail can retry`() = runTest(main.dispatcher) {
        val h = harness()
        backend.failNext(FakeCall.AssignableUsers, BackendError.Network("offline"))
        h.vm.openPicker(PickerKind.Assignees)
        advanceUntilIdle()
        assertInstanceOf(UiState.Error::class.java, h.vm.state.value.picker!!.options)
        h.vm.retryPicker()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, h.vm.state.value.picker!!.options)
    }

    @Test
    fun `labels for the menu and the byline`() = runTest(main.dispatcher) {
        val h = harness()
        assertEquals("Close as not planned", actionLabel(IssueStateAction.Close(CloseIssueAs.NotPlanned)))
        assertEquals("ada-lin opened 3h ago", issueByline(h.issue, "3h ago"))
        val milestone = harness(IssueRef("RhizoNymph/rostrum", 18)).issue
        assertEquals("RhizoNymph opened 2d ago · 0.2", issueByline(milestone, "2d ago"))
    }
}
