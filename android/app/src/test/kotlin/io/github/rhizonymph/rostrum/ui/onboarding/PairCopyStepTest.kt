package io.github.rhizonymph.rostrum.ui.onboarding

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.InMemorySecretStore
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.navigation.OnboardingHold
import io.github.rhizonymph.rostrum.ui.settings.accountSecrets
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

/** After pairing: "Copy settings from <machine>?", or straight on when nothing would change. */
@OptIn(ExperimentalCoroutinesApi::class)
class PairCopyStepTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val link = "rostrum://pair?v=1&m=nymph-desk&h=192.168.1.24&p=8485&c=WDJB-MJHT&fp=AAAA"

    private class Harness(
        val backend: FakeRostrumBackend,
        val session: SessionRepository,
        val vm: PairViewModel,
        val hold: OnboardingHold,
        val messages: MutableList<String>,
    )

    private suspend fun TestScope.harness(
        backend: FakeRostrumBackend = testBackend(signedIn = false, paired = false),
        secrets: InMemorySecretStore = InMemorySecretStore(),
    ): Harness {
        val session = SessionRepository(backend, secrets, "Pixel").also { it.restore() }
        val hold = OnboardingHold()
        val appMessages = Messages()
        val received = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { appMessages.flow.toList(received) }
        val vm = PairViewModel(backend, session, link, hold, appMessages)
        advanceUntilIdle()
        return Harness(backend, session, vm, hold, received)
    }

    private fun TestScope.pair(h: Harness) {
        h.vm.pairWithLink()
        advanceUntilIdle()
    }

    @Test
    fun `when the desktop's settings differ, pairing stops at the offer with the graph held`() = runTest(main.dispatcher) {
        val h = harness()
        pair(h)
        val offer = h.vm.state.value.copy!!
        assertEquals("nymph-desk", offer.preview.machine)
        assertEquals(listOf("serde-rs/serde"), offer.preview.added)
        assertEquals(listOf("rust-lang/rust", "bevyengine/bevy"), offer.preview.removed)
        assertFalse(h.vm.state.value.paired)
        // Pairing signed the phone in, but the first-run screens stay until the question is answered.
        assertTrue((h.session.state.value as SessionState.Ready).github is GitHubAuth.SignedIn)
        assertTrue(h.hold.held.value)
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
        assertFalse(h.hold.held.value)
        assertTrue(h.messages.isEmpty())
    }

    @Test
    fun `a failed preview falls through to the feed with a message`() = runTest(main.dispatcher) {
        val h = harness()
        h.backend.failNext(FakeCall.DesktopConfig, BackendError.DesktopTimeout)
        pair(h)
        assertNull(h.vm.state.value.copy)
        assertTrue(h.vm.state.value.paired)
        assertFalse(h.hold.held.value)
        assertEquals(1, h.messages.size)
        assertTrue(h.messages.single().startsWith("Paired with nymph-desk. Couldn't read its settings"), h.messages.single())
    }

    @Test
    fun `copying replaces the settings, refreshes the feed and finishes`() = runTest(main.dispatcher) {
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
        assertFalse(h.hold.held.value)
    }

    @Test
    fun `keeping this phone's settings changes nothing`() = runTest(main.dispatcher) {
        val h = harness()
        pair(h)
        val before = h.backend.settings().orFail()
        h.vm.keepPhoneSettings()
        advanceUntilIdle()
        assertTrue(h.vm.state.value.paired)
        assertEquals(before, h.backend.settings().orFail())
        assertTrue(h.messages.isEmpty())
        assertFalse(h.hold.held.value)
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
        assertTrue(h.hold.held.value)
        h.vm.keepPhoneSettings()
        assertTrue(h.vm.state.value.paired)
        assertFalse(h.hold.held.value)
    }

    @Test
    fun `pairing from Settings (already signed in) never holds the graph`() = runTest(main.dispatcher) {
        val h = harness(testBackend(signedIn = false, paired = false), accountSecrets(signedIn = true, paired = false))
        pair(h)
        assertTrue(h.vm.state.value.copy != null)
        assertFalse(h.hold.held.value)
    }

    @Test
    fun `a failed pairing releases the hold`() = runTest(main.dispatcher) {
        val h = harness()
        h.backend.failNext(FakeCall.PairWithLink, BackendError.DesktopUnreachable("refused"))
        pair(h)
        assertFalse(h.hold.held.value)
        assertNull(h.vm.state.value.copy)
    }
}
