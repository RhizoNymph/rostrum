package io.github.rhizonymph.rostrum.notifications

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.testing.InMemorySecretStore
import io.github.rhizonymph.rostrum.testing.RecordingBackgroundWork
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class NotificationsTest {
    private val token = "ghp_abcdefghijklmnopqrstuvwxyz0123"

    @Test
    fun `review requests go to their own channel with the author`() {
        val spec = NotificationContent.of(
            NotificationEvent(NotificationKind.ReviewRequested, "RhizoNymph/rostrum", 12, "fix: scroll", "ada-lin", "u"),
        )
        assertEquals(RostrumChannel.ReviewRequests, spec.channel)
        assertEquals("ada-lin asked for your review", spec.title)
        assertEquals("RhizoNymph/rostrum #12 · fix: scroll", spec.text)
        assertEquals(PrRef("RhizoNymph/rostrum", 12), spec.pr)
    }

    @Test
    fun `new pull requests name the repository`() {
        val spec = NotificationContent.of(
            NotificationEvent(NotificationKind.NewPullRequest, "zed-industries/zed", 38150, "OSC 8", null, "u"),
        )
        assertEquals(RostrumChannel.NewPullRequests, spec.channel)
        assertEquals("New pull request in zed-industries/zed", spec.title)
        assertEquals("#38150 OSC 8 · Someone", spec.text)
    }

    @Test
    fun `ids are stable per pull request`() {
        assertEquals(NotificationContent.idOf(PrRef("A/b", 1)), NotificationContent.idOf(PrRef("a/B", 1)))
    }

    @Test
    fun `the check is scheduled only when signed in with a toggle on`() {
        val work = RecordingBackgroundWork()
        val scheduler = NotificationScheduler(work)
        scheduler.sync(signedIn = true, notifyNewPullRequests = false, notifyReviewRequests = true)
        assertEquals(setOf(NotificationScheduler.WORK_NAME), work.scheduled)
        scheduler.sync(signedIn = true, notifyNewPullRequests = false, notifyReviewRequests = false)
        assertTrue(work.scheduled.isEmpty())
        scheduler.sync(signedIn = false, notifyNewPullRequests = true, notifyReviewRequests = true)
        assertTrue(work.scheduled.isEmpty())
    }

    @Test
    fun `a signed-out check posts nothing`() = runTest {
        val posted = mutableListOf<NotificationSpec>()
        val session = SessionRepository(testBackend(signedIn = false), InMemorySecretStore(), "Pixel")
        val result = NotificationCheck(session, testBackend(signedIn = false), posted::add).run()
        assertEquals(CheckResult.SignedOut, result)
        assertTrue(posted.isEmpty())
    }

    @Test
    fun `a check restores the session and posts what is new`() = runTest {
        val backend = testBackend(signedIn = false)
        val session = SessionRepository(backend, InMemorySecretStore(mapOf(SecretKey.GitHubToken to token)), "Pixel")
        val posted = mutableListOf<NotificationSpec>()
        val check = NotificationCheck(session, backend, posted::add)
        assertEquals(CheckResult.Posted(0), check.run())
        assertEquals(CheckResult.Posted(1), check.run())
        assertEquals(RostrumChannel.ReviewRequests, posted.single().channel)
    }

    @Test
    fun `network trouble retries, a revoked token gives up`() = runTest {
        val backend = testBackend(signedIn = false)
        val session = SessionRepository(backend, InMemorySecretStore(mapOf(SecretKey.GitHubToken to token)), "Pixel")
        val check = NotificationCheck(session, backend) {}
        backend.failNext(FakeCall.CheckNotifications, BackendError.Network("offline"))
        assertEquals(CheckResult.Retry(BackendError.Network("offline")), check.run())
        backend.failNext(FakeCall.CheckNotifications, BackendError.GitHubAuthFailed("revoked"))
        assertEquals(CheckResult.GaveUp(BackendError.GitHubAuthFailed("revoked")), check.run())
    }
}
