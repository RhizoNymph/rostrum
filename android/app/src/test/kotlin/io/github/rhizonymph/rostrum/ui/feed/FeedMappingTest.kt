package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.runBlocking
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class FeedMappingTest {
    private val pulls: List<PrSummary> = runBlocking {
        val backend = testBackend()
        backend.clearFilter().orFail().repos.flatMap { section ->
            (section.body as? RepoBody.Pulls)?.pulls.orEmpty()
        } + backend.toggleCollapsed("rust-lang/rust").orFail().repos
            .flatMap { (it.body as? RepoBody.Pulls)?.pulls.orEmpty() }
            .filter { it.repo == "rust-lang/rust" }
    }

    private fun pr(number: Int) = pulls.first { it.number == number }

    @Nested
    inner class Chips {
        @Test
        fun `the diff overview row reads Your review, behind, handed off`() {
            val chips = rowChips(pr(10))
            assertEquals(listOf("Your review", "↓4 main", "handed off"), chips.map { it.text })
            assertEquals(listOf(ColorRole.Accent, ColorRole.Warning, ColorRole.Accent), chips.map { it.role })
            assertTrue(chips[1].mono)
        }

        @Test
        fun `a local neutral chip carries the desktop icon`() {
            val chips = rowChips(pr(9))
            assertEquals(listOf("↓3 main", "↑2 unpushed"), chips.map { it.text })
            assertEquals(ChipIcon.Desktop, chips[1].icon)
            assertNull(chips[0].icon)
        }

        @Test
        fun `conflicts come first`() {
            assertEquals(listOf("conflict", "Your review", "↓2 main"), rowChips(pr(38112)).map { it.text })
        }

        @Test
        fun `approval gets a check icon`() {
            val chips = rowChips(pr(38090))
            assertEquals(listOf("Approved", "↓12 main"), chips.map { it.text })
            assertEquals(ChipIcon.Check, chips[0].icon)
            assertEquals(ColorRole.Success, chips[0].role)
        }

        @Test
        fun `a quiet draft has no chip row`() {
            assertTrue(rowChips(pr(11)).isEmpty())
        }
    }

    @Nested
    inner class Meta {
        @Test
        fun `your own pull requests say you`() {
            assertEquals("you", authorLabel(pr(9)))
            assertEquals("ada-lin", authorLabel(pr(10)))
        }

        @Test
        fun `ages count from when the pull request was opened`() {
            assertEquals("2h", ageLabel(pr(10), TEST_NOW))
            assertEquals("12m", ageLabel(pr(11), TEST_NOW))
        }

        @Test
        fun `zero counts are subdued`() {
            assertEquals(CountTone.Added, additionsTone(3))
            assertEquals(CountTone.Zero, additionsTone(0))
            assertEquals(CountTone.Removed, deletionsTone(9))
            assertEquals(CountTone.Zero, deletionsTone(0))
        }

        @Test
        fun `repository tiles use the name's first letter`() {
            assertEquals("R", repoInitial("RhizoNymph/rostrum"))
            assertEquals("Z", repoInitial("zed-industries/zed"))
            assertEquals("?", repoInitial("owner/"))
        }

        @Test
        fun `headings and footers`() {
            assertEquals("14 open", openCountText(14))
            assertEquals("2 empty repositories hidden", hiddenReposText(2))
            assertEquals("1 empty repository hidden", hiddenReposText(1))
        }

        @Test
        fun `empty bodies say whether the filter is the reason`() {
            val none = RepoSection("a/b", RepoLoad.Loaded(TEST_NOW), 0, 0, false, RepoBody.Empty)
            val filtered = none.copy(openCount = 3)
            assertEquals("No open pull requests", emptyBodyText(none))
            assertEquals("None match the filter", emptyBodyText(filtered))
        }
    }

    @Nested
    inner class Filters {
        private val prefs = FeedPreferences.Default

        @Test
        fun `authors and involved count, involved only with authors`() {
            assertEquals(0, activeFilterCount(prefs))
            assertEquals(0, activeFilterCount(prefs.copy(includeInvolved = true)))
            assertEquals(2, activeFilterCount(prefs.copy(authors = listOf("rhizonymph", "ada-lin"), includeInvolved = true)))
            assertEquals(1, activeFilterCount(prefs.copy(hideDrafts = true)))
            assertEquals(0, activeFilterCount(prefs.copy(hideEmptyRepos = false)))
        }

        @Test
        fun `the authors chip names you as me and folds the rest`() {
            assertEquals("Authors", authorsChipLabel(prefs, "RhizoNymph"))
            assertEquals("Authors: me, ada-lin", authorsChipLabel(prefs.copy(authors = listOf("rhizonymph", "ada-lin")), "RhizoNymph"))
            assertEquals(
                "Authors: me, ada-lin +2",
                authorsChipLabel(prefs.copy(authors = listOf("rhizonymph", "ada-lin", "tjvance", "wren")), "RhizoNymph"),
            )
            assertEquals("Authors: rhizonymph", authorsChipLabel(prefs.copy(authors = listOf("rhizonymph")), null))
        }

        @Test
        fun `the filter button says how many are active`() {
            assertEquals("Feed filters", filterButtonDescription(0))
            assertEquals("Feed filters, 2 active", filterButtonDescription(2))
        }

        @Test
        fun `the sheet heading counts active filters`() {
            assertEquals("2 active", activeText(2))
            assertNull(activeText(0))
        }

        @Test
        fun `author sublines`() {
            assertEquals("you · 2 open", authorSubline(isViewer = true, openPrs = 2))
            assertEquals("1 open", authorSubline(isViewer = false, openPrs = 1))
        }
    }

    @Nested
    inner class Desktop {
        private val machine = runBlocking { testBackend().machineInfo().orFail() }

        @Test
        fun `unpaired phones offer pairing`() {
            assertEquals(DesktopPill.NotPaired, desktopPillOf(paired = false, info = null))
            assertEquals(PillView("Pair desktop", null, "Pair a desktop"), DesktopPill.NotPaired.view())
        }

        @Test
        fun `a desktop that answers is connected`() {
            val pill = desktopPillOf(true, Outcome.Ok(machine))
            assertEquals(DesktopPill.Connected("nymph-desk"), pill)
            assertEquals(PillView("nymph-desk", ColorRole.Success, "Desktop nymph-desk, connected"), pill.view())
        }

        @Test
        fun `timeouts and unreachable desktops warn`() {
            assertEquals(DesktopPill.Unreachable, desktopPillOf(true, Outcome.Err(BackendError.DesktopUnreachable("refused"))))
            assertEquals(DesktopPill.Unreachable, desktopPillOf(true, Outcome.Err(BackendError.DesktopTimeout)))
            assertEquals(ColorRole.Warning, DesktopPill.Unreachable.view().dot)
        }

        @Test
        fun `a revoked pairing needs pairing again`() {
            assertEquals(DesktopPill.NotPaired, desktopPillOf(true, Outcome.Err(BackendError.DeviceRevoked)))
            assertEquals(DesktopPill.NotPaired, desktopPillOf(true, Outcome.Err(BackendError.NotPaired)))
        }

        @Test
        fun `other trouble shows as a problem`() {
            val error = BackendError.RemoteApi(RemoteErrorCode.Internal, "boom")
            val pill = desktopPillOf(true, Outcome.Err(error))
            assertEquals(DesktopPill.Problem(error), pill)
            assertEquals(ColorRole.Danger, pill.view().dot)
        }

        @Test
        fun `while checking the pill is neutral`() {
            assertEquals(DesktopPill.Checking, desktopPillOf(true, null))
            assertEquals(ColorRole.Neutral, DesktopPill.Checking.view().dot)
        }
    }
}
