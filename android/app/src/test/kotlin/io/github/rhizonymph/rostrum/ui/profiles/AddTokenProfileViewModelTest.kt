package io.github.rhizonymph.rostrum.ui.profiles

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testProfileManager
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.settings.TEST_TOKEN
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class AddTokenProfileViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val registry = FakeProfileRegistry()

    @Test
    fun `a named token profile is added and switched to`() = runTest(main.dispatcher) {
        registry.active = registry.seed("nymph-desk")
        val profiles = testProfileManager(registry, InMemorySecretVault()).also { it.start() }
        val vm = AddTokenProfileViewModel(profiles)
        assertFalse(vm.state.value.canSubmit)
        vm.onLabelChange("Work")
        vm.onTokenChange(TEST_TOKEN)
        vm.add()
        advanceUntilIdle()
        val active = (profiles.state.value as ProfilesState.Ready).activeProfile!!
        assertEquals("Work", active.label)
        assertEquals(ProfileKind.TokenOnly, active.kind)
        assertEquals("", vm.state.value.token)
        assertTrue("redacted" in vm.state.value.copy(token = TEST_TOKEN).toString())
    }

    @Test
    fun `a rejected token stays on the form with the error`() = runTest(main.dispatcher) {
        val profiles = testProfileManager(registry, InMemorySecretVault()).also { it.start() }
        val vm = AddTokenProfileViewModel(profiles)
        vm.onTokenChange("nope")
        vm.add()
        advanceUntilIdle()
        assertInstanceOf(BackendError.GitHubAuthFailed::class.java, (vm.state.value.submit as ActionState.Failed).error)
        assertEquals(ProfilesState.Ready(emptyList(), null), profiles.state.value)
    }
}
