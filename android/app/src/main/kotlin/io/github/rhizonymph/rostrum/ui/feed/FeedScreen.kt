package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.requiresSignIn
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView

/**
 * The feed, stateless: header (with [profileLabel], the active profile, in
 * its pill), optional search field, filter chips, and the repository cards
 * under pull-to-refresh.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FeedScreen(
    state: FeedUiState,
    actions: FeedActions,
    modifier: Modifier = Modifier,
    profileLabel: String = "nymph-desk",
) {
    val snapshot = state.feed.dataOrNull()
    Column(modifier.fillMaxSize()) {
        FeedHeader(
            openCount = snapshot?.visibleOpen,
            desktop = state.desktop.view(profileLabel),
            filterActive = snapshot?.filterActive == true,
            filterCount = snapshot?.let { activeFilterCount(it.preferences) } ?: 0,
            searchOpen = state.search is SearchState.Open,
            actions = actions,
        )
        (state.search as? SearchState.Open)?.let { search ->
            FeedSearchField(search.text, actions.queryChange)
        }
        if (snapshot != null) {
            FilterChipsRow(snapshot.preferences, snapshot.viewer?.login, actions)
        }
        PullToRefreshBox(
            isRefreshing = state.refreshing,
            onRefresh = actions.refresh,
            modifier = Modifier.weight(1f).fillMaxWidth(),
        ) {
            when (val feed = state.feed) {
                UiState.Loading -> Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    LoadingView(label = "Loading your feed…")
                }
                is UiState.Error -> Column(
                    Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 12.dp),
                ) {
                    if (feed.error.requiresSignIn) {
                        AuthBanner(feed.error, actions.signOut)
                    } else {
                        ErrorView(feed.error, title = "Couldn't load the feed", onRetry = actions.retry)
                    }
                }
                is UiState.Loaded -> FeedList(feed.data, state.now, state.authProblem, actions)
            }
        }
    }
}
