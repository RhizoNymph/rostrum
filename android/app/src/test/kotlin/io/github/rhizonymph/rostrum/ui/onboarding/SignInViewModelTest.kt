package io.github.rhizonymph.rostrum.ui.onboarding

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.InMemorySecretStore
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.settings.TEST_TOKEN
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

    private val backend = testBackend(signedIn = false, paired = false)

    private suspend fun session(): SessionRepository =
        SessionRepository(backend, InMemorySecretStore(), "Pixel").also { it.restore() }

    @Test
    fun `starts with the token form closed and empty`() = runTest(main.dispatcher) {
        val vm = SignInViewModel(session())
        advanceUntilIdle()
        val state = vm.state.value
        assertFalse(state.tokenFormOpen)
        assertEquals("", state.token)
        assertNull(state.notice)
        assertFalse(state.canSubmit)
    }

    @Test
    fun `an involuntary sign-out notice is shown`() = runTest(main.dispatcher) {
        val session = session()
        session.signOut("Your token was revoked. Sign in again.")
        val vm = SignInViewModel(session)
        advanceUntilIdle()
        assertEquals("Your token was revoked. Sign in again.", vm.state.value.notice)
    }

    @Test
    fun `the token form toggles`() = runTest(main.dispatcher) {
        val vm = SignInViewModel(session())
        vm.toggleTokenForm()
        assertTrue(vm.state.value.tokenFormOpen)
        vm.toggleTokenForm()
        assertFalse(vm.state.value.tokenFormOpen)
    }

    @Test
    fun `a good token signs in`() = runTest(main.dispatcher) {
        val session = session()
        val vm = SignInViewModel(session)
        vm.onTokenChange(TEST_TOKEN)
        assertTrue(vm.state.value.canSubmit)
        vm.signIn()
        runCurrent()
        advanceUntilIdle()
        assertEquals(GitHubAuth.SignedIn, (session.state.value as SessionState.Ready).github)
        assertEquals(ActionState.Idle, vm.state.value.submit)
    }

    @Test
    fun `a rejected token shows inline and editing clears it`() = runTest(main.dispatcher) {
        val vm = SignInViewModel(session())
        vm.onTokenChange("not-a-real-token")
        vm.signIn()
        advanceUntilIdle()
        val failed = vm.state.value.submit as ActionState.Failed
        assertInstanceOf(BackendError.GitHubAuthFailed::class.java, failed.error)
        vm.onTokenChange("ghp_")
        assertEquals(ActionState.Idle, vm.state.value.submit)
    }

    @Test
    fun `network trouble is reported too`() = runTest(main.dispatcher) {
        val vm = SignInViewModel(session())
        backend.failNext(FakeCall.Viewer, BackendError.Network("offline"))
        vm.onTokenChange(TEST_TOKEN)
        vm.signIn()
        advanceUntilIdle()
        assertEquals(ActionState.Failed(BackendError.Network("offline")), vm.state.value.submit)
    }

    @Test
    fun `a blank token cannot be submitted`() = runTest(main.dispatcher) {
        val vm = SignInViewModel(session())
        vm.onTokenChange("   ")
        assertFalse(vm.state.value.canSubmit)
        vm.signIn()
        advanceUntilIdle()
        assertEquals(ActionState.Idle, vm.state.value.submit)
    }
}
