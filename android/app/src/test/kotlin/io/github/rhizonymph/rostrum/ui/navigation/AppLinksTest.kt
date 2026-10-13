package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.pid
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class AppLinksTest {
    private val view = "android.intent.action.VIEW"

    @Test
    fun `a VIEW of a pairing link is a Pair link`() {
        val uri = "rostrum://pair?name=nymph-desk&code=WDJB-MJHT"
        assertEquals(AppLink.Pair(uri), AppLinks.parse(view, uri, null, null))
    }

    @Test
    fun `other schemes and hosts are ignored`() {
        assertNull(AppLinks.parse(view, "https://github.com/pair?x", null, null))
        assertNull(AppLinks.parse(view, "rostrum://pairing?x", null, null))
        assertNull(AppLinks.parse("android.intent.action.MAIN", "rostrum://pair?x", null, null))
    }

    @Test
    fun `a VIEW of a pr or issue link opens it in the active profile`() {
        assertEquals(
            AppLink.OpenPullRequest(PrRef("octo/repo", 42), null),
            AppLinks.parse(view, "rostrum://pr?repo=octo%2Frepo&number=42", null, null),
        )
        assertEquals(
            AppLink.OpenIssue(IssueRef("octo/repo", 7)),
            AppLinks.parse(view, "rostrum://issue?repo=octo/repo&number=7&from=orchestral", null, null),
        )
        // Only a VIEW counts.
        assertNull(AppLinks.parse("android.intent.action.MAIN", "rostrum://pr?repo=a/b&number=1", null, null))
    }

    @Test
    fun `a malformed pr or issue link is Invalid with the reason`() {
        val parsed = AppLinks.parse(view, "rostrum://pr?repo=octo&number=1", null, null)
        val invalid = assertInstanceOf(AppLink.Invalid::class.java, parsed)
        assertEquals("`octo` is not a GitHub repository (expected owner/name)", invalid.reason)
    }

    @Test
    fun `notification extras open the pull request`() {
        assertEquals(
            AppLink.OpenPullRequest(PrRef("RhizoNymph/rostrum", 12)),
            AppLinks.parse("android.intent.action.MAIN", null, "RhizoNymph/rostrum", 12),
        )
        assertNull(AppLinks.parse(null, null, "not-a-repo", 12))
        assertNull(AppLinks.parse(null, null, "a/b", null))
    }

    @Test
    fun `a notification's profile comes along, a malformed one is dropped`() {
        assertEquals(
            AppLink.OpenPullRequest(PrRef("RhizoNymph/rostrum", 12), pid("0123456789abcdef")),
            AppLinks.parse(null, null, "RhizoNymph/rostrum", 12, "0123456789abcdef"),
        )
        assertEquals(
            AppLink.OpenPullRequest(PrRef("RhizoNymph/rostrum", 12), null),
            AppLinks.parse(null, null, "RhizoNymph/rostrum", 12, "../escape"),
        )
    }

    @Nested
    inner class Routing {
        private val at = TEST_NOW
        private fun profile(id: String) = Profile(pid(id), id, ProfileKind.TokenOnly, null, at, at)
        private val state = ProfilesState.Ready(listOf(profile("p1"), profile("p2")), active = pid("p1"))
        private val pr = PrRef("RhizoNymph/rostrum", 12)

        @Test
        fun `pairing links and the active profile's pull requests are shown where they are`() {
            assertEquals(LinkRoute.Show, routeOf(AppLink.Pair("rostrum://pair?c=X"), state))
            assertEquals(LinkRoute.Show, routeOf(AppLink.OpenPullRequest(pr, pid("p1")), state))
            assertEquals(LinkRoute.Show, routeOf(AppLink.OpenPullRequest(pr, null), state))
        }

        @Test
        fun `another profile's pull request switches to it first`() {
            assertEquals(LinkRoute.SwitchFirst(pid("p2")), routeOf(AppLink.OpenPullRequest(pr, pid("p2")), state))
        }

        @Test
        fun `a removed profile's notification is dropped`() {
            assertEquals(LinkRoute.ProfileGone, routeOf(AppLink.OpenPullRequest(pr, pid("p9")), state))
        }

        @Test
        fun `issue and invalid links are handled where they are`() {
            assertEquals(LinkRoute.Show, routeOf(AppLink.OpenIssue(IssueRef("octo/repo", 7)), state))
            assertEquals(LinkRoute.Show, routeOf(AppLink.Invalid("bad"), state))
        }
    }

    @Nested
    inner class Steps {
        private val pr = PrRef("octo/repo", 42)
        private val issue = IssueRef("octo/repo", 7)

        @Test
        fun `a signed-in graph opens pull requests and issues on their screens`() {
            assertEquals(
                LinkStep.Navigate(Destination.PullRequest("octo/repo", 42)),
                stepFor(AppLink.OpenPullRequest(pr), signedIn = true, profile = pid("p1")),
            )
            assertEquals(
                LinkStep.Navigate(Destination.Issue("octo/repo", 7)),
                stepFor(AppLink.OpenIssue(issue), signedIn = true, profile = pid("p1")),
            )
            assertEquals(
                LinkStep.Navigate(Destination.PullRequest("octo/repo", 42)),
                stepFor(AppLink.OpenPullRequest(pr, pid("p1")), signedIn = true, profile = pid("p1")),
            )
        }

        @Test
        fun `pairing links open pairing, signed in or not`() {
            val link = AppLink.Pair("rostrum://pair?code=A")
            assertEquals(LinkStep.Navigate(Destination.Pair("rostrum://pair?code=A")), stepFor(link, signedIn = false, profile = null))
        }

        @Test
        fun `signed out, an item link is answered with a sentence instead`() {
            val step = stepFor(AppLink.OpenIssue(issue), signedIn = false, profile = pid("p1"))
            assertEquals(LinkStep.Say("Sign in to GitHub first, then open the link to octo/repo#7 again."), step)
            val pull = stepFor(AppLink.OpenPullRequest(pr), signedIn = false, profile = null)
            assertEquals(LinkStep.Say("Sign in to GitHub first, then open the link to octo/repo#42 again."), pull)
        }

        @Test
        fun `another profile's pull request waits for the root to switch`() {
            assertEquals(LinkStep.Wait, stepFor(AppLink.OpenPullRequest(pr, pid("p2")), signedIn = true, profile = pid("p1")))
        }

        @Test
        fun `an invalid link says why`() {
            assertEquals(
                LinkStep.Say("Rostrum can't open this link: it has no `repo` parameter"),
                stepFor(AppLink.Invalid("it has no `repo` parameter"), signedIn = true, profile = pid("p1")),
            )
        }
    }

    @Test
    fun `the inbox keeps the newest link until it is consumed`() {
        val inbox = AppLinkInbox()
        val first = AppLink.Pair("rostrum://pair?code=A")
        val second = AppLink.Pair("rostrum://pair?code=B")
        inbox.offer(first)
        inbox.offer(second)
        inbox.consume(first)
        assertEquals(second, inbox.pending.value)
        inbox.consume(second)
        assertNull(inbox.pending.value)
    }
}
