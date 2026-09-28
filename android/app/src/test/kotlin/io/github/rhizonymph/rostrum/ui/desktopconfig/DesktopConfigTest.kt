package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
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
        fun `unpaired, the preview is NotPaired`() = runTest {
            assertEquals(Outcome.Err(BackendError.NotPaired), DesktopConfigCopier(testBackend(paired = false)).preview())
        }
    }
}
