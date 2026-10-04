package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.model.SortOption
import io.github.rhizonymph.rostrum.data.model.SortSettings
import java.time.Instant

/**
 * The feed's two sorts, as `rostrum_core::sort` defines them: keys of a
 * kind (time, text, count) that names the directions and picks the default;
 * choosing another key resets to its default, the same key keeps its
 * direction. Ordering is pure, so it is unit-tested.
 */
internal class FakeSort {
    enum class Kind { Time, Text, Count }

    var repoKey: RepoSortKey = RepoSortKey.Pushed
        private set
    var repoDirection: SortDirection = SortDirection.Descending
        private set
    var itemKey: ItemSortKey = ItemSortKey.Created
        private set
    var itemDirection: SortDirection = SortDirection.Descending
        private set

    fun setRepo(key: RepoSortKey, direction: SortDirection?) {
        repoDirection = direction ?: if (key == repoKey) repoDirection else defaultOf(kindOf(key))
        repoKey = key
    }

    fun setItem(key: ItemSortKey, direction: SortDirection?) {
        itemDirection = direction ?: if (key == itemKey) itemDirection else defaultOf(kindOf(key))
        itemKey = key
    }

    fun settings(): SortSettings = SortSettings(
        repoKey = repoKey,
        repoDirection = repoDirection,
        repoDirectionLabel = directionLabel(kindOf(repoKey), repoDirection),
        itemKey = itemKey,
        itemDirection = itemDirection,
        itemDirectionLabel = directionLabel(kindOf(itemKey), itemDirection),
        summary = "${short(repoKey.name, kindOf(repoKey), repoDirection)} · ${short(itemKey.name, kindOf(itemKey), itemDirection)}",
        repoOptions = RepoSortKey.entries.map { option(it, it.name, kindOf(it)) },
        itemOptions = ItemSortKey.entries.map { option(it, it.name, kindOf(it)) },
    )

    /** What a repository is ordered by. */
    data class RepoFacts(val repo: String, val pushed: Instant, val updated: Instant, val created: Instant, val stars: Int)

    /** What an item (or a stack member) is ordered by. */
    data class ItemFacts(val number: Int, val created: Instant, val updated: Instant, val author: String, val title: String)

    fun orderRepos(repos: List<RepoFacts>): List<String> {
        val comparator: Comparator<RepoFacts> = when (repoKey) {
            RepoSortKey.Pushed -> compareBy { it.pushed }
            RepoSortKey.Updated -> compareBy { it.updated }
            RepoSortKey.Created -> compareBy { it.created }
            RepoSortKey.Owner -> compareBy(String.CASE_INSENSITIVE_ORDER) { it.repo.substringBefore('/') }
            RepoSortKey.Name -> compareBy(String.CASE_INSENSITIVE_ORDER) { it.repo.substringAfter('/') }
            RepoSortKey.Stars -> compareBy { it.stars }
        }
        val directed = if (repoDirection == SortDirection.Descending) comparator.reversed() else comparator
        return repos.sortedWith(directed.thenBy(String.CASE_INSENSITIVE_ORDER) { it.repo }).map { it.repo }
    }

    /**
     * Order [units] (a lone item is a group of one). Text keys compare the
     * bottom member; time keys the newest member descending, the oldest
     * ascending; ties break on the bottom member's number.
     */
    fun <T> orderItems(units: List<T>, members: (T) -> List<ItemFacts>): List<T> {
        val descending = itemDirection == SortDirection.Descending
        fun time(facts: List<ItemFacts>, of: (ItemFacts) -> Instant): Instant =
            if (descending) facts.maxOf(of) else facts.minOf(of)
        val comparator: Comparator<T> = when (itemKey) {
            ItemSortKey.Pushed, ItemSortKey.Updated -> compareBy { time(members(it)) { f -> f.updated } }
            ItemSortKey.Created -> compareBy { time(members(it)) { f -> f.created } }
            ItemSortKey.Author -> compareBy(String.CASE_INSENSITIVE_ORDER) { members(it).first().author }
            ItemSortKey.Title -> compareBy(String.CASE_INSENSITIVE_ORDER) { members(it).first().title }
        }
        val directed = if (descending) comparator.reversed() else comparator
        return units.sortedWith(directed.thenBy { members(it).first().number })
    }

    private fun <K> option(key: K, name: String, kind: Kind) = SortOption(
        key = key,
        label = name.lowercase().replaceFirstChar { it.uppercase() },
        defaultDirection = defaultOf(kind),
        descendingLabel = directionLabel(kind, SortDirection.Descending),
        ascendingLabel = directionLabel(kind, SortDirection.Ascending),
    )

    companion object {
        /** Repository facts the fake derives from its sample data: pushed from pull requests, updated from both kinds. */
        fun repoFacts(repos: List<String>, pulls: List<FakePull>, issues: List<FakeIssue>, started: Instant): List<RepoFacts> =
            repos.mapIndexed { index, repo ->
                val pushed = pulls.filter { it.repo == repo }.maxOfOrNull { it.updatedAt }
                val updated = (pulls.filter { it.repo == repo }.map { it.updatedAt } + issues.filter { it.repo == repo }.map { it.updatedAt }).maxOrNull()
                val fallback = started.minus(java.time.Duration.ofDays(30L + index))
                RepoFacts(
                    repo = repo,
                    pushed = pushed ?: fallback,
                    updated = updated ?: fallback,
                    created = started.minus(java.time.Duration.ofDays(400L - index * 30L)),
                    stars = SamplePulls.stars[repo] ?: 0,
                )
            }

        fun kindOf(key: RepoSortKey): Kind = when (key) {
            RepoSortKey.Pushed, RepoSortKey.Updated, RepoSortKey.Created -> Kind.Time
            RepoSortKey.Owner, RepoSortKey.Name -> Kind.Text
            RepoSortKey.Stars -> Kind.Count
        }

        fun kindOf(key: ItemSortKey): Kind = when (key) {
            ItemSortKey.Pushed, ItemSortKey.Updated, ItemSortKey.Created -> Kind.Time
            ItemSortKey.Author, ItemSortKey.Title -> Kind.Text
        }

        fun defaultOf(kind: Kind): SortDirection = if (kind == Kind.Text) SortDirection.Ascending else SortDirection.Descending

        fun directionLabel(kind: Kind, direction: SortDirection): String = when (kind) {
            Kind.Time -> if (direction == SortDirection.Descending) "Newest first" else "Oldest first"
            Kind.Text -> if (direction == SortDirection.Descending) "Z→A" else "A→Z"
            Kind.Count -> if (direction == SortDirection.Descending) "Most" else "Fewest"
        }

        /** `pushed ↓`, `title A→Z`. */
        fun short(name: String, kind: Kind, direction: SortDirection): String {
            val key = name.lowercase()
            return when (kind) {
                Kind.Text -> "$key ${directionLabel(kind, direction)}"
                Kind.Time, Kind.Count -> "$key ${if (direction == SortDirection.Descending) "↓" else "↑"}"
            }
        }
    }
}
