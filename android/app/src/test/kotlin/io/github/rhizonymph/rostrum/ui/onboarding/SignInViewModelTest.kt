package io.github.rhizonymph.rostrum.ui.onboarding

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testProfileManager
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.settings.TEST_TOKEN
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class SignInViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val registry = FakeProfileRegistry()
    private val vault = InMemorySecretVault()

    private suspend fun TestScope.profiles(): ProfileManager =
        testProfileManager(registry, vault).also { it.start() }

    /** First run: no profile, the token makes one. */
    private suspend fun TestScope.firstRun(): Pair<ProfileManager, SignInViewModel> {
        val profiles = profiles()
        return profiles to SignInViewModel(profiles, profile = null)
    }

    /** The active profile lost its token. */
    private suspend fun TestScope.signedOutProfile(): Pair<ProfileManager, SignInViewModel> {
        registry.active = registry.seed("Work", ProfileKind.TokenOnly)
        val profiles = profiles()
        val handle = profiles.handle(registry.active!!)
        return profiles to SignInViewModel(profiles, handle)
    }

    private fun ProfileManager.ready() = state.value as ProfilesState.Ready

    @Test
    fun `starts with the token form closed and empty`() = runTest(main.dispatcher) {
        val (_, vm) = firstRun()
        advanceUntilIdle()
        val state = vm.state.value
        assertFalse(state.tokenFormOpen)
        assertEquals("", state.token)
        assertNull(state.notice)
        assertNull(state.profileLabel)
        assertFalse(state.canSubmit)
    }

    @Test
    fun `after the upgrade wiped the old sign-in, first run says to pair again`() = runTest(main.dispatcher) {
        val profiles = testProfileManager(registry, vault, legacy = { true }).also { it.start() }
        val vm = SignInViewModel(profiles, profile = null)
        advanceUntilIdle()
        assertEquals(SignInViewModel.UPGRADE_NOTICE, vm.state.value.notice)
        assertTrue(vm.state.value.notice!!.contains("Pair with your desktop"))
    }

    @Test
    fun `on first run a good token makes a profile and switches to it`() = runTest(main.dispatcher) {
        val (profiles, vm) = firstRun()
        vm.onTokenChange(TEST_TOKEN)
        assertTrue(vm.state.value.canSubmit)
        vm.signIn()
        runCurrent()
        advanceUntilIdle()
        val active = profiles.ready().activeProfile!!
        assertEquals(ProfileKind.TokenOnly, active.kind)
        assertEquals("RhizoNymph", active.label)
        assertEquals(ActionState.Idle, vm.state.value.submit)
        assertEquals("", vm.state.value.token)
    }

    @Test
    fun `a profile that lost its token is named, shows why, and signs in again`() = runTest(main.dispatcher) {
        val (profiles, vm) = signedOutProfile()
        val session = profiles.handle(registry.active!!).session
        session.signOut("Your token was revoked. Sign in again.")
        advanceUntilIdle()
        assertEquals("Your token was revoked. Sign in again.", vm.state.value.notice)
        assertEquals("Work", vm.state.value.profileLabel)
        vm.onTokenChange(TEST_TOKEN)
        vm.signIn()
        advanceUntilIdle()
        assertEquals(GitHubAuth.SignedIn, (session.state.value as SessionState.Ready).github)
        // No new profile.
        assertEquals(1, profiles.ready().profiles.size)
    }

    @Test
    fun `the token form toggles`() = runTest(main.dispatcher) {
        val (_, vm) = firstRun()
        vm.toggleTokenForm()
        assertTrue(vm.state.value.tokenFormOpen)
        vm.toggleTokenForm()
        assertFalse(vm.state.value.tokenFormOpen)
    }

    @Test
    fun `a rejected token shows inline, keeps no profile, and editing clears it`() = runTest(main.dispatcher) {
        val (profiles, vm) = firstRun()
        vm.onTokenChange("not-a-real-token")
        vm.signIn()
        advanceUntilIdle()
        val failed = vm.state.value.submit as ActionState.Failed
        assertInstanceOf(BackendError.GitHubAuthFailed::class.java, failed.error)
        assertEquals(ProfilesState.Ready(emptyList(), null), profiles.state.value)
        vm.onTokenChange("ghp_")
        assertEquals(ActionState.Idle, vm.state.value.submit)
    }

    @Test
    fun `network trouble is reported too`() = runTest(main.dispatcher) {
        val (profiles, vm) = signedOutProfile()
        registry.backend(profiles.ready().active!!).failNext(FakeCall.Viewer, BackendError.Network("offline"))
        vm.onTokenChange(TEST_TOKEN)
        vm.signIn()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(BackendError.Network("offline")), vm.state.value.submit)
    }

    @Test
    fun `a blank token cannot be submitted`() = runTest(main.dispatcher) {
        val (_, vm) = firstRun()
        vm.onTokenChange("   ")
        assertFalse(vm.state.value.canSubmit)
        vm.signIn()
        advanceUntilIdle()
        assertEquals(ActionState.Idle, vm.state.value.submit)
        assertFalse("createTokenProfile" in registry.calls)
    }
}
