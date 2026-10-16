package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.data.model.FeedTab
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.requiresSignIn
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.NewIssueFab
import java.time.ZoneId

/**
 * The feed, stateless: header (with [profileLabel], the active profile, in
 * its pill), optional search field, filter chips beside the GitHub budget
 * chip (times in [zone]), and the repository cards under pull-to-refresh.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FeedScreen(
    state: FeedUiState,
    actions: FeedActions,
    modifier: Modifier = Modifier,
    profileLabel: String = "nymph-desk",
    zone: ZoneId = ZoneId.systemDefault(),
) {
    val snapshot = state.feed.dataOrNull()
    Column(modifier.fillMaxSize()) {
        FeedHeader(
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
            FeedTabs(snapshot.tab, snapshot.tabCounts, snapshot.sort.summary, actions)
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
                FilterChipsRow(snapshot.preferences, snapshot.viewer?.login, actions, Modifier.weight(1f))
                budgetChip(snapshot.budget, state.now, zone)?.let { chip ->
                    BudgetChip(chip, actions.openBudget, Modifier.padding(top = 7.dp, end = 12.dp))
                }
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            PullToRefreshBox(
                isRefreshing = state.refreshing,
                onRefresh = actions.refresh,
                modifier = Modifier.fillMaxSize(),
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
            if (snapshot?.tab == FeedTab.Issues) {
                NewIssueFab(actions.newIssue, Modifier.align(Alignment.BottomEnd).padding(16.dp))
            }
        }
    }
}
