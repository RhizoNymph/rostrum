package io.github.rhizonymph.rostrum.data.model

/** Which way a sort runs. */
enum class SortDirection {
    Ascending, Descending;

    val reversed: SortDirection get() = if (this == Ascending) Descending else Ascending
}

/** How repositories can be ordered. Owner and stars are repository-only keys. */
enum class RepoSortKey { Pushed, Updated, Created, Owner, Name, Stars }

/** How pull requests and issues can be ordered (one sort serves both tabs). */
enum class ItemSortKey { Pushed, Updated, Created, Author, Title }

/** A menu entry for a sort key, named by the core ("Created", "Newest first"). */
data class SortOption<K>(
    val key: K,
    val label: String,
    /** What choosing this key starts at. */
    val defaultDirection: SortDirection,
    val descendingLabel: String,
    val ascendingLabel: String,
) {
    fun directionLabel(direction: SortDirection): String = when (direction) {
        SortDirection.Descending -> descendingLabel
        SortDirection.Ascending -> ascendingLabel
    }
}

/** Both saved sorts and the menus to change them. */
data class SortSettings(
    val repoKey: RepoSortKey,
    val repoDirection: SortDirection,
    val repoDirectionLabel: String,
    val itemKey: ItemSortKey,
    val itemDirection: SortDirection,
    val itemDirectionLabel: String,
    /** `pushed ↓ · created ↓`, for the header. */
    val summary: String,
    val repoOptions: List<SortOption<RepoSortKey>>,
    val itemOptions: List<SortOption<ItemSortKey>>,
)
