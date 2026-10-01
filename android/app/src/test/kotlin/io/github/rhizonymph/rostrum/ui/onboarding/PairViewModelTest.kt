package io.github.rhizonymph.rostrum.ui.onboarding

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.pid
import io.github.rhizonymph.rostrum.testing.testProfileManager
import kotlinx.coroutines.test.TestScope
import io.github.rhizonymph.rostrum.ui.common.ActionState
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class PairViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val registry = FakeProfileRegistry()
    private val link = "rostrum://pair?name=nymph-desk&hosts=192.168.1.24,nymph-desk.local&port=8485&fp=4f2a91c07e3b&code=WDJB-MJHT"

    private suspend fun TestScope.profiles(): ProfileManager =
        testProfileManager(registry, InMemorySecretVault()).also { it.start() }

    /** The session of the profile pairing made. */
    private fun ProfileManager.paired() = handle(pid("p1")).session.state.value as SessionState.Ready

    @Test
    fun `a link is previewed without contacting the desktop`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), link)
        assertEquals(LinkState.Reading, vm.state.value.link)
        advanceUntilIdle()
        val preview = vm.state.value.link as LinkState.Preview
        assertEquals("nymph-desk", preview.preview.machine)
        assertEquals("4F2A · 91C0 · 7E3B", preview.preview.fingerprintShort)
        assertEquals(ActionState.Idle, preview.pairing)
        assertFalse(vm.state.value.manualOpen)
    }

    @Test
    fun `pairing from the link pairs, adopts the token and finishes`() = runTest(main.dispatcher) {
        val profiles = profiles()
        val vm = PairViewModel(profiles, link)
        advanceUntilIdle()
        vm.pairWithLink()
        advanceUntilIdle()
        // The fake desktop's settings differ from the new profile's, so it asks first.
        assertNotNull(vm.state.value.copy)
        vm.keepPhoneSettings()
        advanceUntilIdle()
        assertTrue(vm.state.value.paired)
        assertInstanceOf(DesktopLink.Paired::class.java, profiles.paired().desktop)
        assertInstanceOf(GitHubAuth.SignedIn::class.java, profiles.paired().github)
    }

    @Test
    fun `a failed link pairing shows the error and can be retried`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), link)
        advanceUntilIdle()
        registry.failNext("pairDesktopWithLink", BackendError.RemoteApi(RemoteErrorCode.PairingCodeExpired, "expired"))
        vm.pairWithLink()
        advanceUntilIdle()
        val preview = vm.state.value.link as LinkState.Preview
        assertEquals(ActionState.Failed(BackendError.RemoteApi(RemoteErrorCode.PairingCodeExpired, "expired")), preview.pairing)
        assertFalse(vm.state.value.paired)
        vm.pairWithLink()
        advanceUntilIdle()
        assertNotNull(vm.state.value.copy)
    }

    @Test
    fun `an invalid link opens the manual form`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), "rostrum://pair?name=x")
        advanceUntilIdle()
        assertInstanceOf(LinkState.Invalid::class.java, vm.state.value.link)
        assertTrue(vm.state.value.manualOpen)
    }

    @Test
    fun `without a link the manual form starts closed with the default port`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), null)
        assertNull(vm.state.value.link)
        assertFalse(vm.state.value.manualOpen)
        assertEquals("8485", vm.state.value.manual.port)
        vm.openManual()
        assertTrue(vm.state.value.manualOpen)
    }

    @Test
    fun `manual pairing probes, shows the fingerprint, then pairs`() = runTest(main.dispatcher) {
        val profiles = profiles()
        val vm = PairViewModel(profiles, null)
        vm.onHostChange("192.168.1.24")
        vm.onCodeChange("wdjbmjht")
        assertEquals("WDJB-MJHT", vm.state.value.manual.code)
        vm.probe()
        advanceUntilIdle()
        val probed = vm.state.value.manual.step as ManualStep.Probed
        assertEquals("4F2A · 91C0 · 7E3B", probed.probe.fingerprintShort)
        vm.pairManual()
        advanceUntilIdle()
        vm.keepPhoneSettings()
        advanceUntilIdle()
        assertTrue(vm.state.value.paired)
        assertInstanceOf(DesktopLink.Paired::class.java, profiles.paired().desktop)
    }

    @Test
    fun `changing the host forgets the probe but changing the code does not`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), null)
        vm.onHostChange("192.168.1.24")
        vm.probe()
        advanceUntilIdle()
        vm.onCodeChange("WDJB")
        assertInstanceOf(ManualStep.Probed::class.java, vm.state.value.manual.step)
        vm.onHostChange("192.168.1.25")
        assertEquals(ManualStep.Editing, vm.state.value.manual.step)
    }

    @Test
    fun `a bad port is refused without asking the desktop`() = runTest(main.dispatcher) {
        registry.failNext("probeDesktop", BackendError.Internal("should not be called"))
        val vm = PairViewModel(profiles(), null)
        vm.onHostChange("192.168.1.24")
        vm.onPortChange("99999")
        vm.probe()
        advanceUntilIdle()
        val failed = vm.state.value.manual.step as ManualStep.ProbeFailed
        assertInstanceOf(BackendError.InvalidInput::class.java, failed.error)
    }

    @Test
    fun `an unreachable desktop fails the probe`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), null)
        vm.onHostChange("unreachable.local")
        vm.probe()
        advanceUntilIdle()
        assertInstanceOf(BackendError.DesktopUnreachable::class.java, (vm.state.value.manual.step as ManualStep.ProbeFailed).error)
    }

    @Test
    fun `a wrong code fails the pairing but keeps the probe`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), null)
        vm.onHostChange("192.168.1.24")
        vm.onCodeChange("0000-0000")
        vm.probe()
        advanceUntilIdle()
        vm.pairManual()
        advanceUntilIdle()
        val failed = vm.state.value.manual.step as ManualStep.PairFailed
        assertEquals(RemoteErrorCode.PairingCodeExpired, (failed.error as BackendError.RemoteApi).code)
        assertEquals("nymph-desk", failed.probe.machine)
        assertFalse(vm.state.value.paired)
    }

    @Test
    fun `pairing needs a full code`() = runTest(main.dispatcher) {
        val vm = PairViewModel(profiles(), null)
        vm.onHostChange("192.168.1.24")
        vm.probe()
        advanceUntilIdle()
        vm.onCodeChange("WDJ")
        assertFalse(vm.state.value.manual.canPair)
        vm.onCodeChange("WDJBMJHT")
        assertTrue(vm.state.value.manual.canPair)
    }
}
