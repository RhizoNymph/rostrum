package io.github.rhizonymph.rostrum.ui.feed

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.profileViewModel

/**
 * The feed destination: binds [FeedViewModel] to [FeedScreen] and the filter
 * sheet, refreshes on the settings interval while the screen is started, and
 * lets Back close the search field first.
 */
@Composable
fun FeedRoute(
    profileLabel: String,
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenProfiles: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm = profileViewModel { container, profile ->
        FeedViewModel(
            backend = profile.backend,
            session = profile.session.state,
            clock = container.clock,
            signOutAction = { profile.session.signOut() },
        )
    }
    val state by vm.state.collectAsStateWithLifecycle()
    CollectMessages(vm.messages)

    val lifecycleOwner = LocalLifecycleOwner.current
    LaunchedEffect(vm, lifecycleOwner) {
        lifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) { vm.autoRefreshWhileVisible() }
    }
    BackHandler(enabled = state.search is SearchState.Open) { vm.closeSearch() }

    val actions = remember(vm, onOpenPullRequest, onOpenProfiles) {
        FeedActions(
            openPullRequest = onOpenPullRequest,
            openProfiles = onOpenProfiles,
            refresh = vm::refresh,
            retry = vm::retry,
            toggleCollapsed = vm::toggleCollapsed,
            retryRepo = vm::retryRepo,
            openFilters = vm::openFilters,
            toggleInvolved = vm::toggleInvolved,
            toggleDrafts = vm::toggleDrafts,
            openSearch = vm::openSearch,
            closeSearch = vm::closeSearch,
            queryChange = vm::onQueryChange,
            signOut = vm::signOut,
        )
    }
    val sheetActions = remember(vm) {
        FilterSheetActions(
            toggleAuthor = vm::toggleAuthor,
            showAllAuthors = vm::showAllAuthors,
            retryRoster = vm::openFilters,
            setIncludeInvolved = vm::setIncludeInvolved,
            setShowDrafts = vm::setShowDrafts,
            setHideEmpty = vm::setHideEmpty,
            clear = vm::clearFilters,
            done = vm::closeFilters,
        )
    }

    FeedScreen(state, actions, modifier, profileLabel)

    val filters = state.filters
    val preferences = state.feed.dataOrNull()?.preferences
    if (filters is FilterSheetState.Open && preferences != null) {
        FeedFilterSheet(preferences, filters, sheetActions)
    }
}
