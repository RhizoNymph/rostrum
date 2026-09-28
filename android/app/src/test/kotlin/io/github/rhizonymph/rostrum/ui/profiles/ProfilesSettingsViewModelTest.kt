package io.github.rhizonymph.rostrum.ui.profiles

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testProfileManager
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

@OptIn(ExperimentalCoroutinesApi::class)
class ProfilesSettingsViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val registry = FakeProfileRegistry()
    private val desk = registry.seed("nymph-desk", ProfileKind.Desktop("nymph-desk", "4F2A · 91C0 · 7E3B"))
    private val work = registry.seed("Work", ProfileKind.TokenOnly)

    private class Harness(val profiles: ProfileManager, val vm: ProfilesSettingsViewModel, val messages: List<String>) {
        val ready get() = profiles.state.value as ProfilesState.Ready
    }

    private suspend fun TestScope.harness(active: io.github.rhizonymph.rostrum.data.model.ProfileId = desk): Harness {
        registry.active = active
        val profiles = testProfileManager(registry, InMemorySecretVault()).also { it.start() }
        val appMessages = Messages()
        val received = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { appMessages.flow.toList(received) }
        val vm = ProfilesSettingsViewModel(profiles, appMessages)
        advanceUntilIdle()
        return Harness(profiles, vm, received)
    }

    @Test
    fun `renaming starts from the current name and saves the new one`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.startRename(work)
        assertEquals(RenameDialog(work, "Work"), h.vm.state.value.rename)
        h.vm.onRenameChange("Day job")
        h.vm.saveRename()
        advanceUntilIdle()
        assertNull(h.vm.state.value.rename)
        assertEquals("Day job", h.ready.profile(work)!!.label)
        assertEquals("Day job", h.vm.state.value.rows.first { it.id == work }.label)
    }

    @Test
    fun `a blank name can't be saved`() = runTest(main.dispatcher) {
        val h = harness()
        h.vm.startRename(work)
        h.vm.onRenameChange("  ")
        assertFalse(h.vm.state.value.rename!!.canSave)
        h.vm.saveRename()
        advanceUntilIdle()
        assertEquals("Work", h.ready.profile(work)!!.label)
    }

    @Test
    fun `removing a desktop profile says it unpairs and deletes this phone's data`() = runTest(main.dispatcher) {
        val h = harness(active = work)
        h.vm.askRemove(desk)
        val dialog = h.vm.state.value.remove!!
        assertEquals("Remove nymph-desk?", dialog.title)
        assertEquals(
            "This unpairs nymph-desk and deletes this phone's data for it: its repositories, filters, cache, drafts and GitHub token.",
            dialog.body,
        )
        h.vm.confirmRemove()
        advanceUntilIdle()
        assertNull(h.vm.state.value.remove)
        assertEquals(listOf(work), h.ready.profiles.map { it.id })
        assertEquals(work, h.ready.active)
        assertEquals(listOf("Removed nymph-desk"), h.messages)
    }

    @Test
    fun `removing the active profile says which one takes over, then switches to it`() = runTest(main.dispatcher) {
        val h = harness(active = desk)
        h.vm.askRemove(desk)
        assertEquals(
            "This unpairs nymph-desk and deletes this phone's data for it: its repositories, filters, cache, drafts and GitHub token." +
                " Rostrum switches to Work.",
            h.vm.state.value.remove!!.body,
        )
        h.vm.confirmRemove()
        advanceUntilIdle()
        assertEquals(work, h.ready.active)
        assertEquals(listOf("Removed nymph-desk. Now using Work"), h.messages)
    }

    @Test
    fun `removing the last profile goes back to sign-in`() = runTest(main.dispatcher) {
        val h = harness(active = work)
        h.vm.askRemove(desk)
        h.vm.confirmRemove()
        advanceUntilIdle()
        h.vm.askRemove(work)
        assertEquals(
            "This deletes this phone's data for it: its repositories, filters, cache, drafts and GitHub token. You'll be back at sign-in.",
            h.vm.state.value.remove!!.body,
        )
        h.vm.confirmRemove()
        advanceUntilIdle()
        assertEquals(ProfilesState.Ready(emptyList(), null), h.profiles.state.value)
    }

    @Test
    fun `a failed removal stays in the dialog with the error`() = runTest(main.dispatcher) {
        val h = harness()
        registry.failNext("removeProfile", BackendError.Storage("busy"))
        h.vm.askRemove(work)
        h.vm.confirmRemove()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(BackendError.Storage("busy")), h.vm.state.value.remove!!.action)
        h.vm.dismissDialog()
        assertNull(h.vm.state.value.remove)
        assertEquals(2, h.ready.profiles.size)
    }
}
