package io.github.rhizonymph.rostrum.notifications

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.RecordingBackgroundWork
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.pid
import io.github.rhizonymph.rostrum.testing.testProfileManager
import io.github.rhizonymph.rostrum.ui.navigation.AppLink
import io.github.rhizonymph.rostrum.ui.navigation.AppLinks
import io.github.rhizonymph.rostrum.ui.settings.TEST_TOKEN
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class NotificationsTest {
    private val desk = Profile(pid("p1"), "nymph-desk", ProfileKind.Desktop("nymph-desk", "4F2A"), "RhizoNymph", TEST_NOW, TEST_NOW)
    private val work = Profile(pid("p2"), "Work", ProfileKind.TokenOnly, "rhizo-work", TEST_NOW, TEST_NOW)

    @Test
    fun `review requests go to their own channel, named after the profile`() {
        val spec = NotificationContent.of(
            NotificationEvent(NotificationKind.ReviewRequested, "RhizoNymph/rostrum", 12, "fix: scroll", "ada-lin", "u"),
            desk,
        )
        assertEquals(RostrumChannel.ReviewRequests, spec.channel)
        assertEquals("nymph-desk · ada-lin asked for your review", spec.title)
        assertEquals("RhizoNymph/rostrum #12 · fix: scroll", spec.text)
        assertEquals(PrRef("RhizoNymph/rostrum", 12), spec.pr)
        assertEquals(desk.id, spec.profile)
    }

    @Test
    fun `new pull requests name the repository`() {
        val spec = NotificationContent.of(
            NotificationEvent(NotificationKind.NewPullRequest, "zed-industries/zed", 38150, "OSC 8", null, "u"),
            work,
        )
        assertEquals(RostrumChannel.NewPullRequests, spec.channel)
        assertEquals("Work · New pull request in zed-industries/zed", spec.title)
        assertEquals("#38150 OSC 8 · Someone", spec.text)
    }

    @Test
    fun `ids are stable per profile and pull request`() {
        assertEquals(NotificationContent.idOf(desk.id, PrRef("A/b", 1)), NotificationContent.idOf(desk.id, PrRef("a/B", 1)))
        assertNotEquals(NotificationContent.idOf(desk.id, PrRef("a/b", 1)), NotificationContent.idOf(work.id, PrRef("a/b", 1)))
    }

    @Test
    fun `a tap carries the profile, and opening it reads the profile back`() {
        val spec = NotificationContent.of(
            NotificationEvent(NotificationKind.ReviewRequested, "RhizoNymph/rostrum", 12, "fix", "ada-lin", "u"),
            work,
        )
        val tap = NotificationContent.tapExtras(spec)
        assertEquals(NotificationTap("RhizoNymph/rostrum", 12, "p2"), tap)
        assertEquals(
            AppLink.OpenPullRequest(PrRef("RhizoNymph/rostrum", 12), work.id),
            AppLinks.parse("android.intent.action.MAIN", null, tap.repo, tap.number, tap.profile),
        )
    }

    @Test
    fun `the check is scheduled exactly when some profile wants it`() {
        val work = RecordingBackgroundWork()
        val scheduler = NotificationScheduler(work)
        scheduler.sync(wanted = true)
        assertEquals(setOf(NotificationScheduler.WORK_NAME), work.scheduled)
        scheduler.sync(wanted = false)
        assertTrue(work.scheduled.isEmpty())
    }

    private val registry = FakeProfileRegistry()
    private val vault = InMemorySecretVault()

    private fun TestScope.check(posted: MutableList<NotificationSpec>) =
        NotificationCheck(testProfileManager(registry, vault), posted::add)

    @Test
    fun `with no profiles nothing is checked`() = runTest {
        val posted = mutableListOf<NotificationSpec>()
        assertEquals(CheckResult.Checked(emptyMap()), check(posted).run())
        assertTrue(posted.isEmpty())
    }

    @Test
    fun `every signed-in profile with notifications on is checked, each under its own name`() = runTest {
        val first = registry.seed("nymph-desk")
        val second = registry.seed("Work", ProfileKind.TokenOnly)
        val signedOut = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA"))
        val quiet = registry.seed("Quiet", ProfileKind.TokenOnly)
        listOf(first, second, quiet).forEach { vault.seed(it, mapOf(SecretKey.GitHubToken to TEST_TOKEN)) }
        registry.active = first
        registry.backend(quiet).setNotifications(newPullRequests = false, reviewRequests = false)
        val posted = mutableListOf<NotificationSpec>()
        val check = check(posted)

        val firstRun = check.run() as CheckResult.Checked
        assertEquals(ProfileCheck.SignedOut, firstRun.profiles[signedOut])
        assertEquals(ProfileCheck.NotificationsOff, firstRun.profiles[quiet])
        assertEquals(ProfileCheck.Posted(0), firstRun.profiles[first])
        assertEquals(ProfileCheck.Posted(0), firstRun.profiles[second])

        // The fake reports one review request on each profile's second check.
        val secondRun = check.run() as CheckResult.Checked
        assertEquals(2, secondRun.posted)
        assertFalse(secondRun.shouldRetry)
        assertEquals(setOf(first, second), posted.map { it.profile }.toSet())
        assertEquals(
            setOf("nymph-desk · ada-lin asked for your review", "Work · ada-lin asked for your review"),
            posted.map { it.title }.toSet(),
        )
        assertEquals(2, posted.map { it.id }.toSet().size)
    }

    @Test
    fun `one profile's network trouble retries without hiding the others`() = runTest {
        val first = registry.seed("nymph-desk")
        val second = registry.seed("Work", ProfileKind.TokenOnly)
        listOf(first, second).forEach { vault.seed(it, mapOf(SecretKey.GitHubToken to TEST_TOKEN)) }
        val posted = mutableListOf<NotificationSpec>()
        val check = check(posted)
        check.run()
        registry.backend(first).failNext(FakeCall.CheckNotifications, BackendError.Network("offline"))
        val result = check.run() as CheckResult.Checked
        assertEquals(ProfileCheck.Retry(BackendError.Network("offline")), result.profiles[first])
        assertEquals(ProfileCheck.Posted(1), result.profiles[second])
        assertTrue(result.shouldRetry)
        assertEquals(listOf(second), posted.map { it.profile })
    }

    @Test
    fun `a revoked token gives up on that profile`() = runTest {
        val only = registry.seed("nymph-desk")
        vault.seed(only, mapOf(SecretKey.GitHubToken to TEST_TOKEN))
        val check = check(mutableListOf())
        registry.backend(only).failNext(FakeCall.CheckNotifications, BackendError.GitHubAuthFailed("revoked"))
        val result = check.run() as CheckResult.Checked
        assertEquals(ProfileCheck.GaveUp(BackendError.GitHubAuthFailed("revoked")), result.profiles[only])
        assertFalse(result.shouldRetry)
    }

    @Test
    fun `profiles that can't load are reported, not retried`() = runTest {
        registry.failNext("profiles", BackendError.Storage("disk"))
        assertEquals(CheckResult.Unavailable(BackendError.Storage("disk")), check(mutableListOf()).run())
    }
}
