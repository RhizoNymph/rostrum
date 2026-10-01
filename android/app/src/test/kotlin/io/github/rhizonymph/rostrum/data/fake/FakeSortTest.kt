package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class FakeSortTest {
    private fun at(minutesAgo: Long) = TEST_NOW.minusSeconds(minutesAgo * 60)

    @Test
    fun `defaults are repositories by pushed and items by created, newest first`() {
        val settings = FakeSort().settings()
        assertEquals(RepoSortKey.Pushed, settings.repoKey)
        assertEquals(SortDirection.Descending, settings.repoDirection)
        assertEquals(ItemSortKey.Created, settings.itemKey)
        assertEquals("Newest first", settings.itemDirectionLabel)
        assertEquals("pushed ↓ · created ↓", settings.summary)
    }

    @Test
    fun `a new key starts at its own default, the same key keeps its direction`() {
        val sort = FakeSort()
        sort.setItem(ItemSortKey.Created, SortDirection.Ascending)
        sort.setItem(ItemSortKey.Title, null)
        assertEquals(SortDirection.Ascending, sort.itemDirection)
        assertEquals("title A→Z", sort.settings().summary.substringAfter(" · "))
        sort.setItem(ItemSortKey.Title, SortDirection.Descending)
        sort.setItem(ItemSortKey.Title, null)
        assertEquals(SortDirection.Descending, sort.itemDirection)
        sort.setRepo(RepoSortKey.Stars, null)
        assertEquals(SortDirection.Descending, sort.repoDirection)
        assertEquals("Most", sort.settings().repoDirectionLabel)
    }

    @Test
    fun `options name both directions for their kind`() {
        val options = FakeSort().settings().repoOptions.associateBy { it.key }
        assertEquals(RepoSortKey.entries, options.keys.toList())
        assertEquals("Fewest", options.getValue(RepoSortKey.Stars).ascendingLabel)
        assertEquals("Z→A", options.getValue(RepoSortKey.Name).descendingLabel)
        assertEquals(SortDirection.Ascending, options.getValue(RepoSortKey.Owner).defaultDirection)
    }

    @Test
    fun `repositories order by their facts, ties by name`() {
        val sort = FakeSort()
        val facts = listOf(
            FakeSort.RepoFacts("b/zed", at(10), at(10), at(1000), 50),
            FakeSort.RepoFacts("a/rust", at(30), at(5), at(2000), 90),
            FakeSort.RepoFacts("c/bevy", at(10), at(20), at(500), 30),
        )
        assertEquals(listOf("b/zed", "c/bevy", "a/rust"), sort.orderRepos(facts))
        sort.setRepo(RepoSortKey.Stars, null)
        assertEquals(listOf("a/rust", "b/zed", "c/bevy"), sort.orderRepos(facts))
        sort.setRepo(RepoSortKey.Owner, null)
        assertEquals(listOf("a/rust", "b/zed", "c/bevy"), sort.orderRepos(facts))
    }

    @Test
    fun `a group sorts by its newest member descending, its oldest ascending, its bottom for text`() {
        val sort = FakeSort()
        val lone = listOf(FakeSort.ItemFacts(1, at(30), at(30), "zoe", "b lone"))
        val stack = listOf(
            FakeSort.ItemFacts(2, at(60), at(60), "amy", "c bottom"),
            FakeSort.ItemFacts(3, at(5), at(5), "bob", "a top"),
        )
        val units = listOf(lone, stack)
        assertEquals(listOf(stack, lone), sort.orderItems(units) { it })
        sort.setItem(ItemSortKey.Created, SortDirection.Ascending)
        assertEquals(listOf(stack, lone), sort.orderItems(units) { it })
        sort.setItem(ItemSortKey.Title, null)
        assertEquals(listOf(lone, stack), sort.orderItems(units) { it })
        sort.setItem(ItemSortKey.Author, null)
        assertEquals(listOf(stack, lone), sort.orderItems(units) { it })
    }
}
