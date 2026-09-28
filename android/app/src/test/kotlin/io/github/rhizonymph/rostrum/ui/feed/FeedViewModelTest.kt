package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
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

/** The fake, with a controllable update stream and counters on the calls the feed makes on its own. */
private class CountingBackend(val inner: FakeRostrumBackend) : RostrumBackend by inner {
    val updates = MutableSharedFlow<FeedSnapshot>(extraBufferCapacity = 16)
    override val feedUpdates: Flow<FeedSnapshot> = updates
    var refreshes = 0
    var queries = mutableListOf<String>()
    var seen = 0

    override suspend fun refreshFeed(): Outcome<FeedSnapshot> {
        refreshes++
        return inner.refreshFeed()
    }

    override suspend fun setQuery(query: String): Outcome<FeedSnapshot> {
        queries += query
        return inner.setQuery(query)
    }

    override suspend fun markNotificationsSeen(): Outcome<Unit> {
        seen++
        return inner.markNotificationsSeen()
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
class FeedViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val paired = SessionState.Ready(
        GitHubAuth.SignedIn("github.com"),
        DesktopLink.Paired(RemoteStatus.Paired(listOf("h"), 8485, "4F2A · 91C0 · 7E3B", "h")),
    )
    private val session = MutableStateFlow<SessionState>(paired)
    private var signOuts = 0

    private fun viewModel(backend: RostrumBackend) = FeedViewModel(
        backend = backend,
        session = session,
        clock = TEST_CLOCK,
        signOutAction = { signOuts++ },
    )

    private fun FeedViewModel.feed(): FeedSnapshot = state.value.feed.dataOrNull() ?: error("feed not loaded: ${state.value.feed}")

    private fun TestScope.messagesOf(vm: FeedViewModel): List<String> {
        val collected = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.collect { collected += it } }
        return collected
    }

    @Test
    fun `loads the cached feed, then refreshes and marks notifications seen`() = runTest(main.dispatcher) {
        val backend = CountingBackend(testBackend())
        val vm = viewModel(backend)
        assertEquals(UiState.Loading, vm.state.value.feed)
        advanceUntilIdle()
        val rostrum = vm.feed().repos.first { it.repo == "RhizoNymph/rostrum" }
        assertEquals(RepoLoad.Loaded(TEST_NOW), rostrum.load)
        assertEquals(1, backend.refreshes)
        assertEquals(1, backend.seen)
        assertFalse(vm.state.value.refreshing)
    }

    @Test
    fun `a feed that cannot load at all shows the error, and retry recovers`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.CachedFeed, BackendError.Storage("disk"))
        backend.failNext(FakeCall.RefreshFeed, BackendError.Network("offline"))
        val vm = viewModel(backend)
        advanceUntilIdle()
        assertEquals(UiState.Error(BackendError.Network("offline")), vm.state.value.feed)
        vm.retry()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.feed)
    }

    @Test
    fun `a failed refresh keeps the cached feed and says so`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.RefreshFeed, BackendError.Network("offline"))
        val vm = viewModel(backend)
        val messages = messagesOf(vm)
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.feed)
        assertTrue(messages.single().contains("offline"))
    }

    @Test
    fun `background updates apply, older revisions do not`() = runTest(main.dispatcher) {
        val backend = CountingBackend(testBackend())
        val vm = viewModel(backend)
        advanceUntilIdle()
        val current = vm.feed()
        val newer = backend.inner.toggleCollapsed("rust-lang/rust").orFail()
        backend.updates.emit(newer)
        advanceUntilIdle()
        assertEquals(newer.revision, vm.feed().revision)
        backend.updates.emit(current.copy(revision = 0, visibleOpen = 999))
        advanceUntilIdle()
        assertEquals(newer.revision, vm.feed().revision)
    }

    @Test
    fun `collapsing goes through the backend`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        assertEquals(RepoBody.Collapsed, vm.feed().repos.first { it.repo == "rust-lang/rust" }.body)
        vm.toggleCollapsed("rust-lang/rust")
        advanceUntilIdle()
        assertInstanceOf(RepoBody.Pulls::class.java, vm.feed().repos.first { it.repo == "rust-lang/rust" }.body)
    }

    @Test
    fun `the involved and drafts chips flip their preferences`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.toggleInvolved()
        advanceUntilIdle()
        assertFalse(vm.feed().preferences.includeInvolved)
        vm.toggleDrafts()
        advanceUntilIdle()
        assertTrue(vm.feed().preferences.hideDrafts)
        vm.setShowDrafts(true)
        advanceUntilIdle()
        assertFalse(vm.feed().preferences.hideDrafts)
        vm.setIncludeInvolved(true)
        advanceUntilIdle()
        assertTrue(vm.feed().preferences.includeInvolved)
    }

    @Test
    fun `showing empty repositories brings them back`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        assertEquals(2, vm.feed().hiddenEmptyRepos)
        vm.setHideEmpty(false)
        advanceUntilIdle()
        assertEquals(0, vm.feed().hiddenEmptyRepos)
        assertTrue(vm.feed().repos.any { it.repo == "tokio-rs/tokio" })
    }

    @Test
    fun `opening filters loads a capped roster`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.openFilters()
        advanceUntilIdle()
        val open = vm.state.value.filters as FilterSheetState.Open
        val roster = open.roster.dataOrNull()!!
        assertFalse(open.expanded)
        assertTrue(roster.hidden > 0)
        assertEquals("RhizoNymph", roster.authors.first().login)
    }

    @Test
    fun `showing all authors loads everyone`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.openFilters()
        advanceUntilIdle()
        vm.showAllAuthors()
        advanceUntilIdle()
        val open = vm.state.value.filters as FilterSheetState.Open
        assertTrue(open.expanded)
        assertEquals(0, open.roster.dataOrNull()!!.hidden)
    }

    @Test
    fun `toggling an author updates the filter and the roster`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.openFilters()
        advanceUntilIdle()
        vm.toggleAuthor("tjvance")
        advanceUntilIdle()
        assertTrue("tjvance" in vm.feed().preferences.authors)
        val roster = (vm.state.value.filters as FilterSheetState.Open).roster.dataOrNull()!!
        assertTrue(roster.authors.first { it.login == "tjvance" }.selected)
    }

    @Test
    fun `closing the sheet forgets the roster`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.openFilters()
        vm.closeFilters()
        advanceUntilIdle()
        assertEquals(FilterSheetState.Closed, vm.state.value.filters)
    }

    @Test
    fun `clearing resets the filter and the search`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.openSearch()
        vm.onQueryChange("soft")
        advanceUntilIdle()
        vm.clearFilters()
        advanceUntilIdle()
        assertFalse(vm.feed().filterActive)
        assertEquals("", vm.feed().query)
        assertEquals(SearchState.Open(""), vm.state.value.search)
    }

    @Test
    fun `the query is debounced into one backend call`() = runTest(main.dispatcher) {
        val backend = CountingBackend(testBackend())
        val vm = viewModel(backend)
        advanceUntilIdle()
        vm.openSearch()
        vm.onQueryChange("s")
        advanceTimeBy(100)
        vm.onQueryChange("so")
        advanceTimeBy(100)
        vm.onQueryChange("soft")
        assertEquals(SearchState.Open("soft"), vm.state.value.search)
        advanceTimeBy(100)
        assertTrue(backend.queries.isEmpty())
        advanceUntilIdle()
        assertEquals(listOf("soft"), backend.queries)
        assertEquals("soft", vm.feed().query)
    }

    @Test
    fun `closing search clears the query`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.openSearch()
        vm.onQueryChange("soft")
        advanceUntilIdle()
        vm.closeSearch()
        advanceUntilIdle()
        assertEquals(SearchState.Closed, vm.state.value.search)
        assertEquals("", vm.feed().query)
    }

    @Test
    fun `a failed toggle keeps the feed and reports the error`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = viewModel(backend)
        val messages = messagesOf(vm)
        advanceUntilIdle()
        backend.failNext(FakeCall.ToggleCollapsed, BackendError.Internal("boom"))
        vm.toggleCollapsed("rust-lang/rust")
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.feed)
        assertEquals(listOf("Something went wrong: boom"), messages)
    }

    @Test
    fun `a rejected token raises an auth problem that offers signing out`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.RefreshFeed, BackendError.GitHubAuthFailed("Bad credentials"))
        val vm = viewModel(backend)
        advanceUntilIdle()
        assertEquals(BackendError.GitHubAuthFailed("Bad credentials"), vm.state.value.authProblem)
        vm.signOut()
        advanceUntilIdle()
        assertEquals(1, signOuts)
    }

    @Test
    fun `a successful refresh clears the auth problem`() = runTest(main.dispatcher) {
        val backend = testBackend()
        backend.failNext(FakeCall.RefreshFeed, BackendError.GitHubAuthFailed("Bad credentials"))
        val vm = viewModel(backend)
        advanceUntilIdle()
        vm.refresh()
        advanceUntilIdle()
        assertNull(vm.state.value.authProblem)
    }

    @Test
    fun `retrying a repository refreshes just that one`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.retryRepo("zed-industries/zed")
        advanceUntilIdle()
        assertEquals(RepoLoad.Loaded(TEST_NOW), vm.feed().repos.first { it.repo == "zed-industries/zed" }.load)
    }

    @Test
    fun `auto refresh follows the settings interval`() = runTest(main.dispatcher) {
        val backend = CountingBackend(testBackend())
        backend.inner.setRefreshInterval(30)
        val vm = viewModel(backend)
        advanceUntilIdle()
        assertEquals(1, backend.refreshes)
        backgroundScope.launch { vm.autoRefreshWhileVisible() }
        runCurrent()
        advanceTimeBy(29_000)
        assertEquals(1, backend.refreshes)
        advanceTimeBy(2_000)
        assertEquals(2, backend.refreshes)
        advanceTimeBy(30_000)
        assertEquals(3, backend.refreshes)
    }

    @Test
    fun `the desktop pill follows pairing and the desktop's answer`() = runTest(main.dispatcher) {
        val backend = testBackend()
        val vm = viewModel(backend)
        advanceUntilIdle()
        assertEquals(DesktopPill.Connected("nymph-desk"), vm.state.value.desktop)
        session.value = paired.copy(desktop = DesktopLink.NotPaired)
        advanceUntilIdle()
        assertEquals(DesktopPill.NotPaired, vm.state.value.desktop)
        backend.failNext(FakeCall.MachineInfo, BackendError.DesktopUnreachable("refused"))
        session.value = paired
        advanceUntilIdle()
        assertEquals(DesktopPill.Unreachable, vm.state.value.desktop)
    }

    @Test
    fun `pull to refresh shows the spinner until done`() = runTest(main.dispatcher) {
        val vm = viewModel(testBackend())
        advanceUntilIdle()
        vm.refresh()
        runCurrent()
        advanceUntilIdle()
        assertFalse(vm.state.value.refreshing)
    }
}
