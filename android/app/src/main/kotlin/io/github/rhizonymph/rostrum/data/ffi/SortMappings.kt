package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.model.SortOption
import io.github.rhizonymph.rostrum.data.model.SortSettings
import uniffi.rostrum_ffi.FeedTab as FfiFeedTab
import uniffi.rostrum_ffi.ItemSortKey as FfiItemSortKey
import uniffi.rostrum_ffi.ItemSortOption as FfiItemSortOption
import uniffi.rostrum_ffi.RepoSortKey as FfiRepoSortKey
import uniffi.rostrum_ffi.RepoSortOption as FfiRepoSortOption
import uniffi.rostrum_ffi.SortDirection as FfiSortDirection
import uniffi.rostrum_ffi.SortSettings as FfiSortSettings

/* The feed's tab and its two sorts: generated records ↔ model. */

internal fun FfiFeedTab.toModel(): FeedTab = when (this) {
    FfiFeedTab.PULL_REQUESTS -> FeedTab.PullRequests
    FfiFeedTab.ISSUES -> FeedTab.Issues
}

internal fun FeedTab.toFfi(): FfiFeedTab = when (this) {
    FeedTab.PullRequests -> FfiFeedTab.PULL_REQUESTS
    FeedTab.Issues -> FfiFeedTab.ISSUES
}

internal fun FfiSortDirection.toModel(): SortDirection = when (this) {
    FfiSortDirection.ASCENDING -> SortDirection.Ascending
    FfiSortDirection.DESCENDING -> SortDirection.Descending
}

internal fun SortDirection.toFfi(): FfiSortDirection = when (this) {
    SortDirection.Ascending -> FfiSortDirection.ASCENDING
    SortDirection.Descending -> FfiSortDirection.DESCENDING
}

internal fun FfiRepoSortKey.toModel(): RepoSortKey = when (this) {
    FfiRepoSortKey.PUSHED -> RepoSortKey.Pushed
    FfiRepoSortKey.UPDATED -> RepoSortKey.Updated
    FfiRepoSortKey.CREATED -> RepoSortKey.Created
    FfiRepoSortKey.OWNER -> RepoSortKey.Owner
    FfiRepoSortKey.NAME -> RepoSortKey.Name
    FfiRepoSortKey.STARS -> RepoSortKey.Stars
}

internal fun RepoSortKey.toFfi(): FfiRepoSortKey = when (this) {
    RepoSortKey.Pushed -> FfiRepoSortKey.PUSHED
    RepoSortKey.Updated -> FfiRepoSortKey.UPDATED
    RepoSortKey.Created -> FfiRepoSortKey.CREATED
    RepoSortKey.Owner -> FfiRepoSortKey.OWNER
    RepoSortKey.Name -> FfiRepoSortKey.NAME
    RepoSortKey.Stars -> FfiRepoSortKey.STARS
}

internal fun FfiItemSortKey.toModel(): ItemSortKey = when (this) {
    FfiItemSortKey.PUSHED -> ItemSortKey.Pushed
    FfiItemSortKey.UPDATED -> ItemSortKey.Updated
    FfiItemSortKey.CREATED -> ItemSortKey.Created
    FfiItemSortKey.AUTHOR -> ItemSortKey.Author
    FfiItemSortKey.TITLE -> ItemSortKey.Title
}

internal fun ItemSortKey.toFfi(): FfiItemSortKey = when (this) {
    ItemSortKey.Pushed -> FfiItemSortKey.PUSHED
    ItemSortKey.Updated -> FfiItemSortKey.UPDATED
    ItemSortKey.Created -> FfiItemSortKey.CREATED
    ItemSortKey.Author -> FfiItemSortKey.AUTHOR
    ItemSortKey.Title -> FfiItemSortKey.TITLE
}

internal fun FfiRepoSortOption.toModel() =
    SortOption(key.toModel(), label, defaultDirection.toModel(), descendingLabel, ascendingLabel)

internal fun FfiItemSortOption.toModel() =
    SortOption(key.toModel(), label, defaultDirection.toModel(), descendingLabel, ascendingLabel)

internal fun FfiSortSettings.toModel() = SortSettings(
    repoKey = repoKey.toModel(),
    repoDirection = repoDirection.toModel(),
    repoDirectionLabel = repoDirectionLabel,
    itemKey = itemKey.toModel(),
    itemDirection = itemDirection.toModel(),
    itemDirectionLabel = itemDirectionLabel,
    summary = summary,
    repoOptions = repoOptions.map { it.toModel() },
    itemOptions = itemOptions.map { it.toModel() },
)
