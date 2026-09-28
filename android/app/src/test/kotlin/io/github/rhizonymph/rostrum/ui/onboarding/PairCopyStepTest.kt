package io.github.rhizonymph.rostrum.ui.onboarding

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.session.isSignedIn
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.pid
import io.github.rhizonymph.rostrum.testing.testBackend
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
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/**
 * After pairing on first run: "Copy settings from <machine>?", or straight
 * on when nothing would change. The new profile becomes active only once the
 * question is answered.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PairCopyStepTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val link = "rostrum://pair?v=1&m=nymph-desk&h=192.168.1.24&p=8485&c=WDJB-MJHT&fp=AAAA"

    private class Harness(
        val registry: FakeProfileRegistry,
        val profiles: ProfileManager,
        val vm: PairViewModel,
        val messages: MutableList<String>,
    ) {
        /** The profile pairing made (the fake registry numbers them p1, p2, …). */
        val backend: FakeRostrumBackend get() = registry.backend(pid("p1"))
        val active get() = (profiles.state.value as ProfilesState.Ready).active
    }

    private suspend fun TestScope.harness(
        backend: FakeRostrumBackend = testBackend(signedIn = false, paired = false),
    ): Harness {
        val registry = FakeProfileRegistry { backend }
        val profiles = testProfileManager(registry, InMemorySecretVault()).also { it.start() }
        val appMessages = Messages()
        val received = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { appMessages.flow.toList(received) }
        val vm = PairViewModel(profiles, link, appMessages)
        advanceUntilIdle()
        return Harness(registry, profiles, vm, received)
    }

    private fun TestScope.pair(h: Harness) {
        h.vm.pairWithLink()
        advanceUntilIdle()
    }

    @Test
    fun `when the desktop's settings differ, pairing stops at the offer before switching`() = runTest(main.dispatcher) {
        val h = harness()
        pair(h)
        val offer = h.vm.state.value.copy!!
        assertEquals(pid("p1"), offer.profile)
        assertEquals("nymph-desk", offer.preview.machine)
        assertEquals(listOf("serde-rs/serde"), offer.preview.added)
        assertEquals(listOf("rust-lang/rust", "bevyengine/bevy"), offer.preview.removed)
        assertFalse(h.vm.state.value.paired)
        // The new profile is signed in, but not active until the question is answered.
        assertTrue(h.profiles.handle(pid("p1")).session.state.value.isSignedIn)
        assertNull(h.active)
    }

    @Test
    fun `the step is skipped when copying would change nothing`() = runTest(main.dispatcher) {
        val backend = testBackend(signedIn = false, paired = true)
        backend.copyDesktopConfig().orFail()
        backend.clearRemote()
        val h = harness(backend)
        pair(h)
        assertNull(h.vm.state.value.copy)
        assertTrue(h.vm.state.value.paired)
        assertEquals(pid("p1"), h.active)
        assertTrue(h.messages.isEmpty())
    }

    @Test
    fun `a failed preview falls through to the feed with a message`() = runTest(main.dispatcher) {
        val h = harness()
        h.backend.failNext(FakeCall.DesktopConfig, BackendError.DesktopTimeout)
        pair(h)
        assertNull(h.vm.state.value.copy)
        assertTrue(h.vm.state.value.paired)
        assertEquals(pid("p1"), h.active)
        assertEquals(1, h.messages.size)
        assertTrue(h.messages.single().startsWith("Paired with nymph-desk. Couldn't read its settings"), h.messages.single())
    }

    @Test
    fun `copying replaces the profile's settings, refreshes its feed and switches to it`() = runTest(main.dispatcher) {
        val h = harness()
        pair(h)
        val refreshes = h.backend.feedRefreshes
        h.vm.copySettings()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertNull(h.vm.state.value.copy)
        assertEquals(refreshes + 1, h.backend.feedRefreshes)
        assertEquals(
            listOf("RhizoNymph/rostrum", "zed-industries/zed", "tokio-rs/tokio", "serde-rs/serde"),
            h.backend.settings().orFail().repos,
        )
        assertEquals(listOf("Copied 4 repositories from nymph-desk"), h.messages)
        assertEquals(pid("p1"), h.active)
    }

    @Test
    fun `not copying changes nothing but still switches`() = runTest(main.dispatcher) {
        val h = harness()
        pair(h)
        val before = h.backend.settings().orFail()
        h.vm.keepPhoneSettings()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertEquals(before, h.backend.settings().orFail())
        assertTrue(h.messages.isEmpty())
        assertEquals(pid("p1"), h.active)
    }

    @Test
    fun `a failed copy stays on the offer with the error`() = runTest(main.dispatcher) {
        val h = harness()
        pair(h)
        h.backend.failNext(FakeCall.CopyDesktopConfig, BackendError.DeviceRevoked)
        h.vm.copySettings()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(BackendError.DeviceRevoked), h.vm.state.value.copy!!.action)
        assertFalse(h.vm.state.value.paired)
        assertNull(h.active)
        h.vm.keepPhoneSettings()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertEquals(pid("p1"), h.active)
    }

    @Test
    fun `a failed pairing makes no profile and asks nothing`() = runTest(main.dispatcher) {
        val h = harness()
        h.registry.failNext("pairDesktopWithLink", BackendError.DesktopUnreachable("refused"))
        pair(h)
        assertNull(h.vm.state.value.copy)
        assertFalse(h.vm.state.value.paired)
        assertEquals(ProfilesState.Ready(emptyList(), null), h.profiles.state.value)
    }
}
