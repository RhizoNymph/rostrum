package io.github.rhizonymph.rostrum.ui.profiles

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testProfileManager
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.settings.TEST_TOKEN
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
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

@OptIn(ExperimentalCoroutinesApi::class)
class ProfileSwitcherViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val registry = FakeProfileRegistry()
    private val vault = InMemorySecretVault()
    private val desk = registry.seed("nymph-desk", ProfileKind.Desktop("nymph-desk", "4F2A · 91C0 · 7E3B"))
    private val work = registry.seed("Work", ProfileKind.TokenOnly)

    private class Harness(val profiles: ProfileManager, val vm: ProfileSwitcherViewModel, val messages: List<String>)

    private suspend fun TestScope.harness(): Harness {
        vault.seed(desk, mapOf(SecretKey.GitHubToken to TEST_TOKEN))
        registry.active = desk
        val profiles = testProfileManager(registry, vault).also { it.start() }
        val appMessages = Messages()
        val received = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { appMessages.flow.toList(received) }
        val vm = ProfileSwitcherViewModel(profiles, appMessages)
        advanceUntilIdle()
        return Harness(profiles, vm, received)
    }

    @Test
    fun `lists every profile with its kind, login and the active one checked`() = runTest(main.dispatcher) {
        val h = harness()
        assertEquals(
            listOf(
                ProfileRow(desk, "nymph-desk", "nymph-desk", "RhizoNymph", active = true),
                ProfileRow(work, "Work", "GitHub token", null, active = false),
            ),
            h.vm.state.value.rows,
        )
        assertEquals("nymph-desk · @RhizoNymph", h.vm.state.value.rows.first().detail)
        assertEquals("GitHub token", h.vm.state.value.rows.last().detail)
    }

    @Test
    fun `picking another profile switches to it and says so`() = runTest(main.dispatcher) {
        val h = harness()
        assertTrue(h.vm.switchTo(work))
        assertEquals(work, h.vm.state.value.switching)
        advanceUntilIdle()
        assertEquals(work, (h.profiles.state.value as ProfilesState.Ready).active)
        assertNull(h.vm.state.value.switching)
        assertEquals(listOf(work, desk), h.vm.state.value.rows.map { it.id })
        assertTrue(h.vm.state.value.rows.first().active)
        assertEquals(listOf("Switched to Work"), h.messages)
    }

    @Test
    fun `picking the active profile does nothing`() = runTest(main.dispatcher) {
        val h = harness()
        assertFalse(h.vm.switchTo(desk))
        advanceUntilIdle()
        assertFalse("setActiveProfile" in registry.calls.drop(registry.calls.indexOf("profiles") + 3))
        assertTrue(h.messages.isEmpty())
    }

    @Test
    fun `a failed switch stays on the sheet with the error`() = runTest(main.dispatcher) {
        val h = harness()
        registry.failNext("setActiveProfile", BackendError.Storage("disk"))
        h.vm.switchTo(work)
        advanceUntilIdle()
        assertEquals(BackendError.Storage("disk"), h.vm.state.value.error)
        assertEquals(desk, (h.profiles.state.value as ProfilesState.Ready).active)
        assertTrue(h.messages.isEmpty())
    }
}
