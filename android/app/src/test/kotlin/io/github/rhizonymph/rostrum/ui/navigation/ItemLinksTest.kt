package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** The same cases as the desktop's `rostrum_core::link` tests. */
class ItemLinksTest {
    private fun reason(link: String): String {
        val parsed = ItemLinks.parse(link)
        return assertInstanceOf(ItemLink.Invalid::class.java, parsed, "expected $link to be refused").reason
    }

    @Test
    fun `a pull request link with an encoded repo`() {
        assertEquals(ItemLink.Pull(PrRef("octo/repo", 42)), ItemLinks.parse("rostrum://pr?repo=octo%2Frepo&number=42"))
    }

    @Test
    fun `an issue link with a plain slash`() {
        assertEquals(ItemLink.Issue(IssueRef("octo/repo", 7)), ItemLinks.parse("rostrum://issue?repo=octo/repo&number=7"))
    }

    @Test
    fun `parameters come in any order and unknown ones are ignored`() {
        assertEquals(
            ItemLink.Pull(PrRef("a-b/c.d_e-f", 3)),
            ItemLinks.parse("rostrum://pr?from=orchestral&number=3&x&repo=a-b/c.d_e-f&y=%ZZ"),
        )
    }

    @Test
    fun `scheme and host ignore case, a trailing slash and a fragment`() {
        assertEquals(ItemLink.Pull(PrRef("octo/repo", 9)), ItemLinks.parse("  ROSTRUM://PR/?repo=octo%2frepo&number=9#top \n"))
        assertEquals(ItemLink.Issue(IssueRef("octo/repo", 1)), ItemLinks.parse("rostrum://Issue?repo=octo/repo&number=1"))
    }

    @Test
    fun `every encoded character decodes`() {
        assertEquals(ItemLink.Pull(PrRef("octo/repo", 42)), ItemLinks.parse("rostrum://pr?r%65po=%6Fcto%2F%72epo&number=%34%32"))
    }

    @Test
    fun `other schemes and targets are refused`() {
        assertTrue(reason("https://github.com/octo/repo/pull/1").contains("not a rostrum:// link"))
        assertTrue(reason("rostrum:pr?repo=octo/repo&number=1").contains("not a rostrum:// link"))
        assertTrue(reason("rostrum://pair?code=ABCD").contains("rostrum://pair"))
        assertTrue(reason("rostrum://pr/extra?repo=octo/repo&number=1").contains("rostrum://pr/extra"))
    }

    @Test
    fun `missing and repeated parameters are refused`() {
        assertEquals("it has no `repo` parameter", reason("rostrum://pr?number=1"))
        assertEquals("it has no `number` parameter", reason("rostrum://issue?repo=octo/repo"))
        assertEquals("it has no `repo` parameter", reason("rostrum://pr"))
        assertEquals("it gives `repo` more than once", reason("rostrum://pr?repo=a/b&repo=c/d&number=1"))
        assertEquals("it gives `number` more than once", reason("rostrum://pr?repo=a/b&number=1&number=2"))
    }

    @Test
    fun `broken encoding in a known parameter is refused`() {
        val badRepo = "its `repo` parameter is not valid percent-encoding"
        assertEquals(badRepo, reason("rostrum://pr?repo=octo%2&number=1"))
        assertEquals(badRepo, reason("rostrum://pr?repo=octo%zzrepo&number=1"))
        assertEquals("its `number` parameter is not valid percent-encoding", reason("rostrum://pr?repo=a/b&number=%FF"))
    }

    @Test
    fun `repositories outside GitHub's rules are refused`() {
        val bad = listOf(
            "", "octo", "octo/", "/repo", "a/b/c", "octo/..", "octo/.", "-octo/repo", "octo-/repo",
            "oc--to/repo", "oc_to/repo", "octo/re po", "octo/re+po", "octo/re%2Fpo",
            "https://github.com/octo/repo", "octo/repoé",
        )
        for (repo in bad) {
            val encoded = repo.replace("%", "%25").replace(" ", "%20")
            assertEquals(
                "`$repo` is not a GitHub repository (expected owner/name)",
                reason("rostrum://pr?repo=$encoded&number=1"),
                repo,
            )
        }
        assertInstanceOf(ItemLink.Invalid::class.java, ItemLinks.parse("rostrum://pr?repo=${"o".repeat(40)}/r&number=1"))
        assertInstanceOf(ItemLink.Pull::class.java, ItemLinks.parse("rostrum://pr?repo=${"o".repeat(39)}/r&number=1"))
    }

    @Test
    fun `numbers must be positive plain decimals in range`() {
        for (number in listOf("", "0", "-1", "+1", "1.0", "0x10", "2147483648", "one", " 1")) {
            val encoded = number.replace(" ", "%20").replace("+", "%2B")
            assertEquals(
                "`$number` is not a pull request or issue number",
                reason("rostrum://issue?repo=octo/repo&number=$encoded"),
                number,
            )
        }
        assertEquals(
            ItemLink.Issue(IssueRef("octo/repo", Int.MAX_VALUE)),
            ItemLinks.parse("rostrum://issue?repo=octo/repo&number=2147483647"),
        )
    }

    @Test
    fun `overlong links are refused`() {
        val link = "rostrum://pr?repo=octo/repo&number=1&pad=" + "x".repeat(ItemLinks.MAX_LINK_LENGTH)
        assertTrue(reason(link).contains("longer than"))
    }

    @Test
    fun `only pr and issue links are item links`() {
        assertTrue(ItemLinks.isItemLink("rostrum://pr?repo=a/b&number=1"))
        assertTrue(ItemLinks.isItemLink("rostrum://ISSUE"))
        assertFalse(ItemLinks.isItemLink("rostrum://pair?code=X"))
        assertFalse(ItemLinks.isItemLink("https://example.com/pr"))
    }
}
