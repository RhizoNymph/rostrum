package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.model.SortOption
import io.github.rhizonymph.rostrum.data.model.TabCounts
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/** The feed's Pull requests | Issues tabs and its Sort sheet. */
@OptIn(ExperimentalCoroutinesApi::class)
class FeedTabsSortTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val backend = testBackend()
    private val session = MutableStateFlow<SessionState>(SessionState.Ready(GitHubAuth.SignedIn, DesktopLink.NotPaired))

    private fun TestScope.loaded(): FeedViewModel {
        val vm = FeedViewModel(backend, session, TEST_CLOCK, signOutAction = {})
        advanceUntilIdle()
        return vm
    }

    private fun FeedViewModel.feed(): FeedSnapshot = state.value.feed.dataOrNull()!!

    @Test
    fun `switching to Issues shows issues and keeps both counts`() = runTest(main.dispatcher) {
        val vm = loaded()
        assertEquals(FeedTab.PullRequests, vm.feed().tab)
        vm.selectTab(FeedTab.Issues)
        advanceUntilIdle()
        assertEquals(FeedTab.Issues, vm.feed().tab)
        assertTrue(vm.feed().repos.any { it.body is RepoBody.Issues })
        assertEquals(vm.feed().tabCounts.issues, vm.feed().visibleOpen)
        assertTrue(vm.feed().tabCounts.pullRequests > 0)
    }

    @Test
    fun `the tab is the core's, so it is there on the next load`() = runTest(main.dispatcher) {
        loaded().selectTab(FeedTab.Issues)
        advanceUntilIdle()
        assertEquals(FeedTab.Issues, loaded().feed().tab)
    }

    @Test
    fun `switching tabs with the filter sheet open reloads its roster for that tab`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.openFilters()
        advanceUntilIdle()
        vm.selectTab(FeedTab.Issues)
        advanceUntilIdle()
        val roster = ((vm.state.value.filters as FilterSheetState.Open).roster as UiState.Loaded).data
        assertTrue(roster.authors.any { it.login == "wren" })
    }

    @Test
    fun `a failed tab switch keeps the tab and says why`() = runTest(main.dispatcher) {
        val vm = loaded()
        val messages = mutableListOf<String>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.collect { messages += it } }
        backend.failNext(FakeCall.SetFeedTab, BackendError.Storage("disk"))
        vm.selectTab(FeedTab.Issues)
        advanceUntilIdle()
        assertEquals(FeedTab.PullRequests, vm.feed().tab)
        assertTrue(messages.single().contains("disk"))
    }

    @Test
    fun `the sort sheet opens and closes`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.openSort()
        assertTrue(vm.state.value.sortOpen)
        vm.closeSort()
        assertFalse(vm.state.value.sortOpen)
    }

    @Test
    fun `choosing a key resets to its default, the direction toggle reverses it`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.chooseItemSort(ItemSortKey.Title)
        advanceUntilIdle()
        assertEquals(ItemSortKey.Title, vm.feed().sort.itemKey)
        assertEquals(SortDirection.Ascending, vm.feed().sort.itemDirection)
        vm.setItemSortDirection(SortDirection.Descending)
        advanceUntilIdle()
        assertEquals("Z→A", vm.feed().sort.itemDirectionLabel)
        vm.chooseRepoSort(RepoSortKey.Name)
        advanceUntilIdle()
        assertEquals(listOf("RhizoNymph/rostrum", "zed-industries/zed").sortedBy { it.substringAfter('/') },
            vm.feed().repos.map { it.repo }.filter { it in setOf("RhizoNymph/rostrum", "zed-industries/zed") })
        vm.setRepoSortDirection(SortDirection.Descending)
        advanceUntilIdle()
        assertEquals("zed-industries/zed", vm.feed().repos.first().repo)
        assertEquals("name Z→A · title Z→A", vm.feed().sort.summary)
    }

    @Test
    fun `direction choices start at the key's default and tab names read naturally`() {
        val option = SortOption(ItemSortKey.Title, "Title", SortDirection.Ascending, "Z→A", "A→Z")
        assertEquals(listOf(SortDirection.Ascending, SortDirection.Descending), directionChoices(option))
        assertEquals("Issues", tabTitle(FeedTab.Issues))
        assertEquals("Pull requests, 6 open", tabDescription(FeedTab.PullRequests, TabCounts(6, 2)))
        val empty = RepoSection("a/b", RepoLoad.Idle, 0, 0, false, RepoBody.Empty)
        assertEquals("No open issues", emptyBodyText(empty, FeedTab.Issues))
        assertEquals("None match the filter", emptyBodyText(empty.copy(openCount = 2), FeedTab.Issues))
    }
}
