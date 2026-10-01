package io.github.rhizonymph.rostrum.ui.onboarding

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
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
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/**
 * A pairing link while a profile is already in use: a new desktop gets its
 * own profile and the offer to switch to it; a desktop already paired is
 * re-paired in its profile and nothing switches.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PairProfileBranchesTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val registry = FakeProfileRegistry()
    private val vault = InMemorySecretVault()
    private val newDesktop = "rostrum://pair?v=1&m=framework&h=192.168.1.30&p=8485&c=WDJB-MJHT&fp=aaaabbbbcccc"
    private val knownDesktop = "rostrum://pair?v=1&m=nymph-desk&h=192.168.1.24&p=8485&c=WDJB-MJHT&fp=4f2a91c07e3b"

    private class Harness(val profiles: ProfileManager, val vm: PairViewModel, val messages: List<String>, val current: ProfileId) {
        val ready get() = profiles.state.value as ProfilesState.Ready
    }

    /** Signed in to "nymph-desk" (fingerprint 4F2A · 91C0 · 7E3B), then a link arrives. */
    private suspend fun TestScope.harness(link: String): Harness {
        val current = registry.seed("nymph-desk", ProfileKind.Desktop("nymph-desk", "4F2A · 91C0 · 7E3B"))
        vault.seed(current, mapOf(SecretKey.GitHubToken to TEST_TOKEN))
        registry.active = current
        val profiles = testProfileManager(registry, vault).also { it.start() }
        val appMessages = Messages()
        val received = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { appMessages.flow.toList(received) }
        val vm = PairViewModel(profiles, link, appMessages)
        advanceUntilIdle()
        vm.pairWithLink()
        advanceUntilIdle()
        return Harness(profiles, vm, received, current)
    }

    @Test
    fun `a new desktop makes a profile and offers to switch to it`() = runTest(main.dispatcher) {
        val h = harness(newDesktop)
        val offer = h.vm.state.value.switchOffer!!
        assertEquals("framework", offer.machine)
        assertEquals("framework", offer.profile.label)
        assertEquals("nymph-desk", offer.current)
        assertNull(h.vm.state.value.copy)
        assertFalse(h.vm.state.value.paired)
        assertEquals(2, h.ready.profiles.size)
        assertEquals(h.current, h.ready.active)
    }

    @Test
    fun `switching asks about the new profile's settings, then switches`() = runTest(main.dispatcher) {
        val h = harness(newDesktop)
        val created = h.vm.state.value.switchOffer!!.profile.id
        h.vm.switchToPaired()
        advanceUntilIdle()
        assertNull(h.vm.state.value.switchOffer)
        val copy = h.vm.state.value.copy!!
        assertEquals(created, copy.profile)
        // Still on the old profile while the question is open.
        assertEquals(h.current, h.ready.active)
        h.vm.copySettings()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertEquals(created, h.ready.active)
        assertEquals(listOf("Copied 4 repositories from ${copy.preview.machine}"), h.messages)
        // The copy went to the new profile, not the one in use.
        assertEquals(4, registry.backend(created).settings().orFail().repos.size)
        assertEquals(5, registry.backend(h.current).settings().orFail().repos.size)
    }

    @Test
    fun `switching when the settings can't be read still switches`() = runTest(main.dispatcher) {
        val h = harness(newDesktop)
        val created = h.vm.state.value.switchOffer!!.profile.id
        registry.backend(created).failNext(FakeCall.DesktopConfig, BackendError.DesktopTimeout)
        h.vm.switchToPaired()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertEquals(created, h.ready.active)
    }

    @Test
    fun `staying keeps the current profile and says where to find the new one`() = runTest(main.dispatcher) {
        val h = harness(newDesktop)
        h.vm.stayOnCurrent()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertNull(h.vm.state.value.switchOffer)
        assertEquals(h.current, h.ready.active)
        assertEquals(2, h.ready.profiles.size)
        assertEquals(listOf("Paired with framework. Switch to it from the profile menu."), h.messages)
    }

    @Test
    fun `a known desktop is re-paired in its profile and nothing switches`() = runTest(main.dispatcher) {
        val h = harness(knownDesktop)
        assertNull(h.vm.state.value.switchOffer)
        assertNull(h.vm.state.value.copy)
        assertTrue(h.vm.state.value.paired)
        assertEquals(listOf("Re-paired nymph-desk"), h.messages)
        assertEquals(listOf(h.current), h.ready.profiles.map { it.id })
        assertEquals(h.current, h.ready.active)
        assertNotNull(vault.of(h.current).values[SecretKey.DeviceToken])
    }

    @Test
    fun `re-pairing another profile's desktop stays in the current profile`() = runTest(main.dispatcher) {
        val other = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
        val h = harness(newDesktop)
        assertTrue(h.vm.state.value.paired)
        assertEquals(listOf("Re-paired framework"), h.messages)
        assertEquals(h.current, h.ready.active)
        assertNotNull(vault.of(other).values[SecretKey.DeviceToken])
    }
}
