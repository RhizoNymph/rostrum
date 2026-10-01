package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.runtime.Immutable
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection

/** Everything the feed screen can ask for; the route binds these to the ViewModel. */
@Immutable
class FeedActions(
    val openPullRequest: (PrRef) -> Unit = {},
    val openIssue: (IssueRef) -> Unit = {},
    /** The repository's own screen (its header's name). */
    val openRepo: (repo: String) -> Unit = {},
    val newIssue: () -> Unit = {},
    val selectTab: (FeedTab) -> Unit = {},
    val openSort: () -> Unit = {},
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

/** What the Sort sheet can ask for. */
@Immutable
class SortSheetActions(
    val chooseRepoKey: (RepoSortKey) -> Unit = {},
    val setRepoDirection: (SortDirection) -> Unit = {},
    val chooseItemKey: (ItemSortKey) -> Unit = {},
    val setItemDirection: (SortDirection) -> Unit = {},
    val done: () -> Unit = {},
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
