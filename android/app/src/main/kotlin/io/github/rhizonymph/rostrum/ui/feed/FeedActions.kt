package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.runtime.Immutable
import io.github.rhizonymph.rostrum.data.model.PrRef

/** Everything the feed screen can ask for; the route binds these to the ViewModel. */
@Immutable
class FeedActions(
    val openPullRequest: (PrRef) -> Unit = {},
    val openProfiles: () -> Unit = {},
    val refresh: () -> Unit = {},
    val retry: () -> Unit = {},
    val toggleCollapsed: (repo: String) -> Unit = {},
    val retryRepo: (repo: String) -> Unit = {},
    val openFilters: () -> Unit = {},
    val toggleInvolved: () -> Unit = {},
    val toggleDrafts: () -> Unit = {},
    val openSearch: () -> Unit = {},
    val closeSearch: () -> Unit = {},
    val queryChange: (String) -> Unit = {},
    val signOut: () -> Unit = {},
)

/** What the filter sheet can ask for. */
@Immutable
class FilterSheetActions(
    val toggleAuthor: (login: String) -> Unit = {},
    val showAllAuthors: () -> Unit = {},
    val retryRoster: () -> Unit = {},
    val setIncludeInvolved: (Boolean) -> Unit = {},
    val setShowDrafts: (Boolean) -> Unit = {},
    val setHideEmpty: (Boolean) -> Unit = {},
    val clear: () -> Unit = {},
    val done: () -> Unit = {},
)
