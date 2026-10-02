package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/** Settings › Fetching: pull requests and issues per repository. */
class FetchLimitsTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private fun SettingsViewModel.content(): SettingsContent =
        (state.value.content as? UiState.Loaded)?.data ?: throw AssertionError("not loaded: ${state.value.content}")

    @Test
    fun `both limits load and save`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = SettingsViewModel(backend, restoredSession(backend, accountSecrets(paired = true))) {}
        advanceUntilIdle()
        assertEquals(30, vm.content().prsPerRepo)
        assertEquals(25, vm.content().issuesPerRepo)
        vm.setIssuesPerRepo(50)
        vm.setPrsPerRepo(10)
        advanceUntilIdle()
        assertEquals(50, vm.content().issuesPerRepo)
        assertEquals(10, vm.content().prsPerRepo)
        assertEquals(50, backend.settings().orFail().issuesPerRepo)
        assertEquals(10, backend.settings().orFail().prsPerRepo)
    }

    @Test
    fun `a failed save keeps the old value and says why`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = SettingsViewModel(backend, restoredSession(backend, accountSecrets(paired = true))) {}
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.toList(messages) }
        advanceUntilIdle()
        backend.failNext(FakeCall.SetIssuesPerRepo, BackendError.Storage("disk full"))
        vm.setIssuesPerRepo(50)
        advanceUntilIdle()
        assertEquals(25, vm.content().issuesPerRepo)
        assertEquals(listOf("Local storage failed: disk full"), messages)
    }

    @Test
    fun `the choices include the current value`() {
        assertEquals(listOf(10, 25, 50, 100), FetchLimitChoices.forValue(25))
        assertEquals(listOf(10, 25, 30, 50, 100), FetchLimitChoices.forValue(30))
    }
}
