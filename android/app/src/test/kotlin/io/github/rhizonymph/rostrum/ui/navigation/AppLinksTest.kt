package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.PrRef
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
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
