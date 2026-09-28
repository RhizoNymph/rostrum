package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.pid
import org.junit.jupiter.api.Assertions.assertEquals
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
