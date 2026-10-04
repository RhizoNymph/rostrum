package io.github.rhizonymph.rostrum.ui.newissue

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.PickerKind
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
class NewIssueViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val backend = testBackend()
    private val rostrum = "RhizoNymph/rostrum"

    private class Harness(val vm: NewIssueViewModel, val appMessages: List<String>)

    private fun TestScope.harness(repo: String? = null): Harness {
        val app = Messages()
        val vm = NewIssueViewModel(backend, repo, app)
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { app.flow.collect { messages += it } }
        advanceUntilIdle()
        return Harness(vm, messages)
    }

    @Test
    fun `offers the watched repositories and needs one and a title`() = runTest(main.dispatcher) {
        val h = harness()
        assertEquals(5, (h.vm.state.value.repos as UiState.Loaded).data.size)
        assertNull(h.vm.state.value.repo)
        h.vm.onTitleChange("Crash on rotate")
        assertFalse(h.vm.state.value.canSubmit)
        h.vm.chooseRepo(rostrum)
        assertTrue(h.vm.state.value.canSubmit)
        h.vm.onTitleChange("   ")
        assertFalse(h.vm.state.value.canSubmit)
    }

    @Test
    fun `a preset repository is chosen from the start`() = runTest(main.dispatcher) {
        assertEquals(rostrum, harness(rostrum).vm.state.value.repo)
    }

    @Test
    fun `labels and assignees are picked locally and dropped with a new repository`() = runTest(main.dispatcher) {
        val h = harness(rostrum)
        h.vm.openPicker(PickerKind.Labels)
        advanceUntilIdle()
        h.vm.toggle("bug")
        h.vm.toggle("ui")
        h.vm.toggle("ui")
        assertEquals(setOf("bug"), h.vm.state.value.pickerSelection)
        h.vm.closePicker()
        h.vm.openPicker(PickerKind.Assignees)
        advanceUntilIdle()
        h.vm.toggle("ada-lin")
        assertEquals(setOf("ada-lin"), h.vm.state.value.assignees)
        h.vm.chooseRepo(rostrum)
        assertEquals(setOf("bug"), h.vm.state.value.labels)
        h.vm.chooseRepo("zed-industries/zed")
        assertTrue(h.vm.state.value.labels.isEmpty())
        assertTrue(h.vm.state.value.assignees.isEmpty())
    }

    @Test
    fun `preview renders the body with the core`() = runTest(main.dispatcher) {
        val h = harness(rostrum)
        h.vm.onBodyChange("Some **bold**")
        h.vm.showMode(BodyMode.Preview)
        assertEquals(BodyMode.Preview, h.vm.state.value.mode)
        assertTrue(h.vm.state.value.preview.isNotEmpty())
        h.vm.showMode(BodyMode.Write)
        assertEquals("Some **bold**", h.vm.state.value.body)
    }

    @Test
    fun `submitting opens the issue and leaves for it`() = runTest(main.dispatcher) {
        val h = harness(rostrum)
        h.vm.onTitleChange(" Crash on rotate ")
        h.vm.onBodyChange("steps")
        h.vm.openPicker(PickerKind.Labels)
        advanceUntilIdle()
        h.vm.toggle("bug")
        h.vm.closePicker()
        h.vm.submit()
        advanceUntilIdle()
        assertEquals(IssueRef(rostrum, 22), h.vm.state.value.created)
        assertEquals(listOf("Opened #22 in $rostrum"), h.appMessages)
        val issue = backend.issueDetail(IssueRef(rostrum, 22)).orFail().issue
        assertEquals("Crash on rotate", issue.title)
        assertEquals(listOf("bug"), issue.labels.map { it.name })
    }

    @Test
    fun `a failed submit stays on the form with the error`() = runTest(main.dispatcher) {
        val h = harness(rostrum)
        h.vm.onTitleChange("Crash")
        backend.failNext(FakeCall.CreateIssue, BackendError.GitHubApi(410, "Issues are disabled"))
        h.vm.submit()
        advanceUntilIdle()
        assertInstanceOf(ActionState.Failed::class.java, h.vm.state.value.submit)
        assertNull(h.vm.state.value.created)
        h.vm.onTitleChange("Crash!")
        assertEquals(ActionState.Idle, h.vm.state.value.submit)
    }

    @Test
    fun `chosen lists read naturally`() {
        assertEquals("None", chosenText(emptySet()))
        assertEquals("bug, ui", chosenText(setOf("ui", "bug")))
    }
}
