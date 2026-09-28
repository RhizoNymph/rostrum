package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class DesktopConfigTest {
    private val preview = DesktopConfigPreview(
        machine = "framework",
        repos = listOf("a/one", "b/two", "c/three"),
        added = listOf("c/three"),
        removed = listOf("x/gone", "y/gone"),
        prsPerRepo = 25,
        hideDrafts = true,
        hideEmptyRepos = false,
        authors = listOf("ada-lin", "rhizonymph"),
        includeInvolved = true,
        autostash = true,
        changesAnything = true,
    )

    @Nested
    inner class Text {
        @Test
        fun `titles and the copied message count repositories`() {
            assertEquals("Copy settings from framework?", DesktopConfigText.offerTitle("framework"))
            assertEquals("Copied 7 repositories from framework", DesktopConfigText.copiedMessage("framework", 7))
            assertEquals("Copied 1 repository from framework", DesktopConfigText.copiedMessage("framework", 1))
        }

        @Test
        fun `the removal warning appears only when something is removed`() {
            assertNull(DesktopConfigText.removalWarning(0))
            assertEquals("1 repository will be removed from this phone.", DesktopConfigText.removalWarning(1))
            assertEquals("2 repositories will be removed from this phone.", DesktopConfigText.removalWarning(2))
        }

        @Test
        fun `the preferences summary is one line`() {
            assertEquals(
                "25 per repository · drafts hidden · empty repositories shown · ada-lin, rhizonymph and involved · stash on",
                DesktopConfigText.preferencesSummary(preview),
            )
            val everyone = preview.copy(authors = emptyList(), hideDrafts = false, hideEmptyRepos = true, autostash = false)
            assertEquals(
                "25 per repository · drafts shown · empty repositories hidden · everyone · stash off",
                DesktopConfigText.preferencesSummary(everyone),
            )
        }

        private val phone = Settings(
            repos = listOf("a/one", "b/two", "c/three"),
            refreshIntervalSecs = 60,
            prsPerRepo = 25,
            notifyNewPullRequests = false,
            notifyReviewRequests = false,
            autostash = true,
            feed = FeedPreferences(hideDrafts = true, hideEmptyRepos = false, authors = listOf("ada-lin", "rhizonymph"), includeInvolved = true),
        )

        @Test
        fun `changes list additions and removals`() {
            assertEquals(
                listOf("Adds 1 repository", "Removes 2 repositories"),
                DesktopConfigText.changeLines(preview, phone.copy(repos = listOf("a/one", "b/two", "x/gone", "y/gone"))),
            )
        }

        @Test
        fun `a reorder alone is said as one`() {
            val sameSet = preview.copy(added = emptyList(), removed = emptyList())
            assertEquals(
                listOf("Reorders your repositories to match framework"),
                DesktopConfigText.changeLines(sameSet, phone.copy(repos = listOf("c/three", "a/one", "b/two"))),
            )
        }

        @Test
        fun `feed settings and the stash default are named when they differ`() {
            val sameRepos = preview.copy(added = emptyList(), removed = emptyList())
            assertEquals(
                listOf("Changes pull requests per repository (10 → 25), feed filters and the stash default"),
                DesktopConfigText.changeLines(
                    sameRepos,
                    phone.copy(prsPerRepo = 10, autostash = false, feed = phone.feed.copy(hideDrafts = false)),
                ),
            )
            assertEquals(emptyList<String>(), DesktopConfigText.changeLines(sameRepos, phone))
        }

        @Test
        fun `without this phone's settings, an unexplained change is still admitted`() {
            val sameSet = preview.copy(added = emptyList(), removed = emptyList())
            assertEquals(
                listOf("Reorders your repositories or changes feed settings to match framework"),
                DesktopConfigText.changeLines(sameSet, null),
            )
            assertEquals(emptyList<String>(), DesktopConfigText.changeLines(sameSet.copy(changesAnything = false), null))
        }

        @Test
        fun `repository rows mark what copying adds`() {
            val rows = DesktopConfigText.repoRows(preview)
            assertEquals(listOf("a/one", "b/two", "c/three"), rows.map { it.repo })
            assertEquals(listOf(false, false, true), rows.map { it.added })
        }
    }

    @Nested
    inner class Copier {
        @Test
        fun `copying replaces the settings, refreshes the feed and says how many`() = runTest {
            val backend = testBackend()
            val copier = DesktopConfigCopier(backend)
            val before = backend.feedRefreshes
            val message = copier.copy("nymph-desk").orFail()
            assertEquals("Copied 4 repositories from nymph-desk", message)
            assertEquals(before + 1, backend.feedRefreshes)
            assertFalse(backend.desktopConfig().orFail().changesAnything)
        }

        @Test
        fun `a failed refresh after copying still counts as copied`() = runTest {
            val backend = testBackend()
            backend.failNext(FakeCall.RefreshFeed, BackendError.Network("offline"))
            assertTrue(DesktopConfigCopier(backend).copy("nymph-desk") is Outcome.Ok)
            assertEquals(4, backend.settings().orFail().repos.size)
        }

        @Test
        fun `a failed copy is returned and nothing refreshes`() = runTest {
            val backend = testBackend()
            backend.failNext(FakeCall.CopyDesktopConfig, BackendError.DesktopUnreachable("refused"))
            val before = backend.feedRefreshes
            assertEquals(Outcome.Err(BackendError.DesktopUnreachable("refused")), DesktopConfigCopier(backend).copy("nymph-desk"))
            assertEquals(before, backend.feedRefreshes)
        }

        @Test
        fun `the preview carries what copying would change on this phone`() = runTest {
            val offer = DesktopConfigCopier(testBackend()).preview().orFail()
            assertEquals("nymph-desk", offer.preview.machine)
            assertEquals(
                listOf("Adds 1 repository", "Removes 2 repositories", "Changes pull requests per repository (30 → 25), feed filters and the stash default"),
                offer.changes,
            )
        }

        @Test
        fun `unpaired, the preview is NotPaired`() = runTest {
            assertEquals(Outcome.Err(BackendError.NotPaired), DesktopConfigCopier(testBackend(paired = false)).preview())
        }
    }
}
