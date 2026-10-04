package io.github.rhizonymph.rostrum.ui.items

import io.github.rhizonymph.rostrum.data.fake.FakeStacks
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.StackKind
import io.github.rhizonymph.rostrum.data.model.StackSummary
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test

class ItemRowsTest {
    @Test
    fun `stack places run bottom to top`() {
        assertEquals(StackPlace.Only, StackPlace.of(0, 1))
        assertEquals(listOf(StackPlace.Bottom, StackPlace.Middle, StackPlace.Top), (0..2).map { StackPlace.of(it, 3) })
    }

    @Test
    fun `a stack becomes its header then its members, a lone pull request one row`() = runTest {
        val feed = testBackend().clearFilter().orFail()
        val section = feed.repos.first { it.repo == "RhizoNymph/rostrum" }
        val rows = rowsOf(section.repo, (section.body as RepoBody.Pulls).items)
        val header = rows.first() as ItemRow.StackHeader
        assertEquals("Stack 7 · 2 PRs", header.stack.title)
        assertEquals(listOf(9 to StackPlace.Bottom, 11 to StackPlace.Top, 10 to null),
            rows.drop(1).map { (it as ItemRow.Pull).let { pull -> pull.pr.number to pull.stack } })
        assertEquals(rows.size, rows.map { it.key }.toSet().size)
    }

    @Test
    fun `stack sublines name the trunk and members not open`() {
        val stack = StackSummary(StackKind.Chain, "Stackable chain · 3 PRs", "main", 3, 0, null)
        assertEquals("on main", stackSubline(stack))
        assertEquals("on main · 2 not open", stackSubline(stack.copy(absent = 2)))
    }

    @Test
    fun `comment counts and assignees read naturally`() = runTest {
        assertNull(commentsText(0))
        assertEquals("1 comment", commentsText(1))
        assertEquals("4 comments", commentsText(4))
        val issue = testBackend().issueDetail(io.github.rhizonymph.rostrum.data.model.IssueRef("zed-industries/zed", 20410)).orFail().issue
        assertEquals("ada-lin", assigneesText(issue))
        assertEquals("tjvance", issueAuthorLabel(issue))
    }

    @Test
    fun `an empty stack item still has a key`() {
        val item = PullItem.Stack(FakeStacks.summary(FakeStacks.samples.first(), emptyList()), emptyList())
        assertEquals("stack:Stack 7 · 0 PRs", item.key)
    }
}
