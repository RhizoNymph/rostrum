package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.CloneInfo
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.TabCounts
import io.github.rhizonymph.rostrum.testing.TEST_SORT
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource

class SettingsMappingTest {
    private val machine = MachineInfo(
        name = "nymph-desk", version = "0.1.0", apiVersion = 1,
        clones = listOf(CloneInfo("a/one", "~/Code/one")),
        handlerConfigured = true, autostash = false,
    )

    private fun feed(shown: List<String>, hideEmpty: Boolean = true) = FeedSnapshot(
        revision = 1,
        tab = FeedTab.PullRequests,
        tabCounts = TabCounts(0, 0),
        sort = TEST_SORT,
        repos = shown.map { RepoSection(it, RepoLoad.Idle, 0, 0, false, RepoBody.Empty) },
        hiddenEmptyRepos = 0, totalOpen = 0, visibleOpen = 0, query = "",
        preferences = FeedPreferences.Default.copy(hideEmptyRepos = hideEmpty),
        filterActive = false, mergeStatesSettling = false, viewer = null,
    )

    @Test
    fun `clones, missing clones and hidden repositories`() {
        val rows = repoRows(listOf("a/one", "b/two", "c/three"), feed(listOf("a/one", "b/two")), machine)
        assertEquals(
            listOf(
                RepoRow("a/one", RepoDetail.Clone("nymph-desk", "~/Code/one")),
                RepoRow("b/two", RepoDetail.NoClone),
                RepoRow("c/three", RepoDetail.HiddenEmpty),
            ),
            rows,
        )
    }

    @Test
    fun `nothing is hidden when empty repositories are shown`() {
        val rows = repoRows(listOf("c/three"), feed(emptyList(), hideEmpty = false), null)
        assertEquals(RepoDetail.None, rows.single().detail)
    }

    @Test
    fun `without a feed nothing is known to be hidden`() {
        assertEquals(RepoDetail.NoClone, repoRows(listOf("x/y"), null, machine).single().detail)
    }

    @Test
    fun `sublines`() {
        assertEquals("clone on nymph-desk · ~/Code/one", RepoDetail.Clone("nymph-desk", "~/Code/one").subline())
        assertEquals("no clone", RepoDetail.NoClone.subline())
        assertEquals("no open pull requests · hidden", RepoDetail.HiddenEmpty.subline())
        assertNull(RepoDetail.None.subline())
    }

    @ParameterizedTest
    @CsvSource("30, every 30 s", "60, every 60 s", "300, every 5 min", "900, every 15 min", "90, every 90 s", "3600, every 60 min")
    fun `interval labels`(seconds: Long, expected: String) {
        assertEquals(expected, refreshIntervalLabel(seconds))
    }

    @Test
    fun `add errors name the repository in mono`() {
        assertEquals(
            InlineError(code = "rust-lang/rust", rest = " is already in your feed"),
            addRepoError(BackendError.DuplicateRepo("rust-lang/rust")),
        )
        assertEquals(
            InlineError(code = "nope", rest = " isn't a repository: expected owner/name"),
            addRepoError(BackendError.InvalidRepo("nope", "expected owner/name")),
        )
        assertEquals(InlineError(code = null, rest = "Couldn't reach GitHub: offline"), addRepoError(BackendError.Network("offline")))
    }

    @Test
    fun `account sublines`() {
        assertEquals("github.com · signed in with a token", accountSubline("github.com"))
    }
}
