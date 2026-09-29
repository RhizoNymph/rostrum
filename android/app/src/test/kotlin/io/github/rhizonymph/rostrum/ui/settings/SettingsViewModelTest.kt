package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class SettingsViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val changed = mutableListOf<Settings>()

    private suspend fun TestScope.viewModel(
        paired: Boolean = true,
        backend: io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend = testBackend(paired = paired),
    ): Pair<SettingsViewModel, io.github.rhizonymph.rostrum.data.session.SessionRepository> {
        val session = restoredSession(backend, accountSecrets(paired = paired))
        val vm = SettingsViewModel(backend, session) { changed += it }
        advanceUntilIdle()
        return vm to session
    }

    private fun SettingsViewModel.content(): SettingsContent =
        (state.value.content as? UiState.Loaded)?.data ?: throw AssertionError("not loaded: ${state.value.content}")

    @Test
    fun `loads the account, repositories, desktop and sync settings`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val content = vm.content()
        assertEquals(AccountViewer.Known("RhizoNymph"), content.account.viewer)
        assertEquals("github.com", content.account.host)
        assertEquals(
            listOf("RhizoNymph/rostrum", "zed-industries/zed", "rust-lang/rust", "tokio-rs/tokio", "bevyengine/bevy"),
            content.repos.map { it.repo },
        )
        assertEquals(RepoDetail.Clone("nymph-desk", "~/Code/devtools/rostrum"), content.repos[0].detail)
        assertEquals(RepoDetail.NoClone, content.repos[2].detail)
        assertEquals(RepoDetail.HiddenEmpty, content.repos[3].detail)
        assertEquals("nymph-desk", (content.desktop as DesktopSummary.Connected).machine.name)
        assertEquals(60, content.refreshIntervalSecs)
        assertTrue(content.notifyNewPullRequests)
        assertTrue(content.notifyReviewRequests)
    }

    @Test
    fun `an unpaired phone shows no clone details`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel(paired = false)
        val content = vm.content()
        assertEquals(DesktopSummary.NotPaired, content.desktop)
        assertEquals(RepoDetail.None, content.repos[0].detail)
        assertEquals(RepoDetail.HiddenEmpty, content.repos[3].detail)
    }

    @Test
    fun `an unreachable desktop is reported in the desktop row only`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.MachineInfo, BackendError.DesktopUnreachable("timeout"))
        val (vm, _) = viewModel(backend = backend)
        assertEquals(DesktopSummary.Unreachable(BackendError.DesktopUnreachable("timeout")), vm.content().desktop)
    }

    @Test
    fun `a failed viewer lookup still shows the settings`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.Viewer, BackendError.Network("offline"))
        val (vm, _) = viewModel(backend = backend)
        assertEquals(AccountViewer.Unknown(BackendError.Network("offline")), vm.content().account.viewer)
    }

    @Test
    fun `failing to load settings is an error that retry clears`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.Settings, BackendError.Storage("disk"))
        val (vm, _) = viewModel(backend = backend)
        assertEquals(UiState.Error(BackendError.Storage("disk")), vm.state.value.content)
        vm.retry()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.content)
    }

    @Test
    fun `adding a repository clears the field, lists it and says so`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val message = backgroundScope.launch { assertEquals("Added serde-rs/serde", vm.messages.flow.first()) }
        vm.onAddInputChange("https://github.com/serde-rs/serde")
        vm.addRepo()
        advanceUntilIdle()
        assertEquals(AddRepoState(), vm.state.value.addRepo)
        assertTrue(vm.content().repos.any { it.repo == "serde-rs/serde" })
        message.join()
    }

    @Test
    fun `a duplicate shows inline and editing clears it`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        vm.onAddInputChange("rust-lang/rust")
        vm.addRepo()
        advanceUntilIdle()
        assertEquals(BackendError.DuplicateRepo("rust-lang/rust"), vm.state.value.addRepo.error)
        assertEquals("rust-lang/rust", vm.state.value.addRepo.input)
        vm.onAddInputChange("rust-lang/rus")
        assertNull(vm.state.value.addRepo.error)
    }

    @Test
    fun `malformed input shows inline`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        vm.onAddInputChange("not a repo")
        vm.addRepo()
        advanceUntilIdle()
        assertInstanceOf(BackendError.InvalidRepo::class.java, vm.state.value.addRepo.error)
    }

    @Test
    fun `a blank field adds nothing`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        vm.addRepo()
        advanceUntilIdle()
        assertEquals(5, vm.content().repos.size)
        assertNull(vm.state.value.addRepo.error)
    }

    @Test
    fun `other add failures go to the snackbar, not the field`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val (vm, _) = viewModel(backend = backend)
        backend.failNext(FakeCall.AddRepo, BackendError.Storage("disk full"))
        val message = backgroundScope.launch { assertTrue(vm.messages.flow.first().contains("disk full")) }
        vm.onAddInputChange("serde-rs/serde")
        vm.addRepo()
        advanceUntilIdle()
        assertNull(vm.state.value.addRepo.error)
        assertFalse(vm.state.value.addRepo.running)
        message.join()
    }

    @Test
    fun `removing a repository drops its row`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val message = backgroundScope.launch { assertEquals("Removed tokio-rs/tokio", vm.messages.flow.first()) }
        vm.removeRepo("tokio-rs/tokio")
        advanceUntilIdle()
        assertFalse(vm.content().repos.any { it.repo == "tokio-rs/tokio" })
        assertTrue(vm.state.value.removing.isEmpty())
        message.join()
    }

    @Test
    fun `notification toggles save and resync the schedule`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        vm.setNotifyReviewRequests(false)
        advanceUntilIdle()
        assertFalse(vm.content().notifyReviewRequests)
        assertTrue(vm.content().notifyNewPullRequests)
        assertFalse(changed.single().notifyReviewRequests)
        vm.setNotifyNewPullRequests(false)
        advanceUntilIdle()
        assertFalse(vm.content().notifyNewPullRequests)
        assertEquals(2, changed.size)
    }

    @Test
    fun `a failed toggle keeps the old value and does not resync`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val (vm, _) = viewModel(backend = backend)
        backend.failNext(FakeCall.SetNotifications, BackendError.Storage("disk"))
        vm.setNotifyNewPullRequests(false)
        advanceUntilIdle()
        assertTrue(vm.content().notifyNewPullRequests)
        assertTrue(changed.isEmpty())
    }

    @Test
    fun `the refresh interval changes`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        vm.setRefreshInterval(300)
        advanceUntilIdle()
        assertEquals(300, vm.content().refreshIntervalSecs)
    }

    @Test
    fun `signing out ends the session`() = runTest(main.dispatcher) {
        val (vm, session) = viewModel()
        vm.signOut()
        advanceUntilIdle()
        assertInstanceOf(GitHubAuth.SignedOut::class.java, (session.state.value as SessionState.Ready).github)
    }

    @Test
    fun `unpairing elsewhere updates the desktop row`() = runTest(main.dispatcher) {
        val (vm, session) = viewModel()
        session.forgetDesktop()
        advanceUntilIdle()
        assertEquals(DesktopSummary.NotPaired, vm.content().desktop)
    }

    @Test
    fun `a token from the desktop is reported`() = runTest(main.dispatcher) {
        val (vm, _) = viewModel()
        val message = backgroundScope.launch { assertTrue(vm.messages.flow.first().contains("nymph-desk")) }
        vm.refreshTokenFromDesktop()
        advanceUntilIdle()
        message.join()
    }
}
