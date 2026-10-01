package io.github.rhizonymph.rostrum.ui.feed

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.requiresSignIn
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isPaired
import io.github.rhizonymph.rostrum.data.valueOrNull
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.toUiState
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Clock

/**
 * The feed: paints the cached snapshot, refreshes, follows background
 * updates (keeping the highest revision), and drives the tabs, the Sort
 * sheet, the filter chips, the search box and the filter sheet. Every change goes through the backend,
 * which answers with the next snapshot.
 */
@OptIn(FlowPreview::class)
class FeedViewModel(
    private val backend: RostrumBackend,
    private val session: StateFlow<SessionState>,
    private val clock: Clock,
    private val signOutAction: suspend () -> Unit,
    queryDebounceMillis: Long = QUERY_DEBOUNCE_MILLIS,
) : ViewModel() {
    private val _state = MutableStateFlow(FeedUiState(now = clock.instant()))
    val state: StateFlow<FeedUiState> = _state.asStateFlow()

    private val outbox = Messages()

    /** One-shot snackbar messages. */
    val messages: Flow<String> = outbox.flow

    /** What the user typed, before the debounce hands it to the core. */
    private val typedQuery = MutableSharedFlow<String>(extraBufferCapacity = 16)

    init {
        viewModelScope.launch { backend.feedUpdates.collect(::apply) }
        viewModelScope.launch { typedQuery.debounce(queryDebounceMillis).collect(::applyQuery) }
        viewModelScope.launch {
            session.map { it.isPaired }.distinctUntilChanged().collectLatest(::loadDesktop)
        }
        viewModelScope.launch { load() }
    }

    // --- loading -------------------------------------------------------------------

    private suspend fun load() {
        when (val cached = backend.cachedFeed()) {
            is Outcome.Ok -> apply(cached.value)
            is Outcome.Err -> RostrumLog.w(TAG, "feed_cache_unavailable", "error" to cached.error::class.simpleName)
        }
        refreshNow(spinner = false)
    }

    /** Keep a snapshot unless a newer one is already shown. */
    private fun apply(snapshot: FeedSnapshot) {
        _state.update { current ->
            val shown = current.feed.dataOrNull()
            if (shown != null && snapshot.revision < shown.revision) {
                current
            } else {
                current.copy(feed = UiState.Loaded(snapshot), now = clock.instant())
            }
        }
    }

    private suspend fun refreshNow(spinner: Boolean) {
        if (spinner) _state.update { it.copy(refreshing = true) }
        when (val result = backend.refreshFeed().logErr(TAG, "feed_refresh_failed")) {
            is Outcome.Ok -> {
                apply(result.value)
                _state.update { it.copy(authProblem = null) }
                backend.markNotificationsSeen().logErr(TAG, "mark_seen_failed")
                RostrumLog.d(TAG, "feed_refreshed", "revision" to result.value.revision, "visible" to result.value.visibleOpen)
            }
            is Outcome.Err -> refreshFailed(result.error)
        }
        if (spinner) _state.update { it.copy(refreshing = false) }
    }

    private fun refreshFailed(error: BackendError) {
        val loaded = _state.value.feed is UiState.Loaded
        _state.update { state ->
            state.copy(
                authProblem = if (error.requiresSignIn) error else state.authProblem,
                feed = if (loaded) state.feed else UiState.Error(error),
            )
        }
        if (loaded && !error.requiresSignIn) outbox.send("Couldn't refresh. ${error.describe()}")
    }

    /** Pull to refresh: the feed and the desktop pill. */
    fun refresh() {
        if (_state.value.refreshing) return
        viewModelScope.launch {
            refreshNow(spinner = true)
            loadDesktop(session.value.isPaired)
        }
    }

    /** After a failed first load. */
    fun retry() {
        _state.update { it.copy(feed = UiState.Loading) }
        viewModelScope.launch { load() }
    }

    /**
     * Refresh every `refreshIntervalSecs` for as long as the caller runs it;
     * the screen runs it while it is started (`repeatOnLifecycle`).
     */
    suspend fun autoRefreshWhileVisible() {
        while (true) {
            val seconds = backend.settings().valueOrNull()?.refreshIntervalSecs ?: DEFAULT_REFRESH_SECONDS
            delay(seconds * 1_000)
            refreshNow(spinner = false)
        }
    }

    private suspend fun loadDesktop(paired: Boolean) {
        if (!paired) {
            _state.update { it.copy(desktop = DesktopPill.NotPaired) }
            return
        }
        _state.update { it.copy(desktop = desktopPillOf(true, null)) }
        val info = backend.machineInfo().logErr(TAG, "desktop_check_failed")
        _state.update { it.copy(desktop = desktopPillOf(true, info)) }
    }

    // --- changes -------------------------------------------------------------------

    private fun change(event: String, vararg fields: Pair<String, Any?>, call: suspend () -> Outcome<FeedSnapshot>) {
        viewModelScope.launch {
            when (val result = call()) {
                is Outcome.Ok -> {
                    apply(result.value)
                    RostrumLog.i(TAG, event, *fields)
                }
                is Outcome.Err -> failed(event, result.error)
            }
        }
    }

    private fun failed(event: String, error: BackendError) {
        RostrumLog.w(TAG, "${event}_failed", "error" to error::class.simpleName)
        if (error.requiresSignIn) _state.update { it.copy(authProblem = error) }
        outbox.send(error.describe())
    }

    private fun preferences(): FeedPreferences? = _state.value.feed.dataOrNull()?.preferences

    private fun setPreferences(event: String, transform: (FeedPreferences) -> FeedPreferences) {
        val current = preferences() ?: return
        val next = transform(current)
        change(event, "preferences" to next) { backend.setFilter(next) }
    }

    fun toggleCollapsed(repo: String) = change("repo_collapse_toggled", "repo" to repo) { backend.toggleCollapsed(repo) }

    fun retryRepo(repo: String) = change("repo_refreshed", "repo" to repo) { backend.refreshRepo(repo) }

    fun toggleInvolved() = setPreferences("filter_involved") { it.copy(includeInvolved = !it.includeInvolved) }

    fun setIncludeInvolved(include: Boolean) = setPreferences("filter_involved") { it.copy(includeInvolved = include) }

    fun toggleDrafts() = setPreferences("filter_drafts") { it.copy(hideDrafts = !it.hideDrafts) }

    fun setShowDrafts(show: Boolean) = setPreferences("filter_drafts") { it.copy(hideDrafts = !show) }

    fun setHideEmpty(hide: Boolean) = setPreferences("filter_hide_empty") { it.copy(hideEmptyRepos = hide) }

    fun toggleAuthor(login: String) {
        viewModelScope.launch {
            when (val result = backend.toggleAuthor(login)) {
                is Outcome.Ok -> {
                    apply(result.value)
                    RostrumLog.i(TAG, "filter_author_toggled", "login" to login)
                    reloadRoster()
                }
                is Outcome.Err -> failed("filter_author_toggled", result.error)
            }
        }
    }

    /** Reset every saved preference and the search. */
    fun clearFilters() {
        _state.update { state ->
            state.copy(search = (state.search as? SearchState.Open)?.copy(text = "") ?: state.search)
        }
        viewModelScope.launch {
            when (val result = backend.clearFilter()) {
                is Outcome.Ok -> {
                    apply(result.value)
                    RostrumLog.i(TAG, "filter_cleared")
                    reloadRoster()
                }
                is Outcome.Err -> failed("filter_cleared", result.error)
            }
        }
    }

    // --- tabs and sort -------------------------------------------------------------

    /** Show pull requests or issues; the core persists the choice. */
    fun selectTab(tab: FeedTab) {
        if (_state.value.feed.dataOrNull()?.tab == tab) return
        viewModelScope.launch {
            when (val result = backend.setFeedTab(tab)) {
                is Outcome.Ok -> {
                    apply(result.value)
                    RostrumLog.i(TAG, "feed_tab", "tab" to tab)
                    reloadRoster()
                }
                is Outcome.Err -> failed("feed_tab", result.error)
            }
        }
    }

    fun openSort() {
        _state.update { it.copy(sortOpen = true) }
    }

    fun closeSort() {
        _state.update { it.copy(sortOpen = false) }
    }

    /** A new key starts at its default direction; the core decides it. */
    fun chooseRepoSort(key: RepoSortKey) = change("repo_sort", "key" to key) { backend.setRepoSort(key, null) }

    fun setRepoSortDirection(direction: SortDirection) {
        val key = _state.value.feed.dataOrNull()?.sort?.repoKey ?: return
        change("repo_sort", "key" to key, "direction" to direction) { backend.setRepoSort(key, direction) }
    }

    fun chooseItemSort(key: ItemSortKey) = change("item_sort", "key" to key) { backend.setItemSort(key, null) }

    fun setItemSortDirection(direction: SortDirection) {
        val key = _state.value.feed.dataOrNull()?.sort?.itemKey ?: return
        change("item_sort", "key" to key, "direction" to direction) { backend.setItemSort(key, direction) }
    }

    // --- search --------------------------------------------------------------------

    fun openSearch() {
        _state.update { state ->
            if (state.search is SearchState.Open) state else state.copy(search = SearchState.Open(state.feed.dataOrNull()?.query.orEmpty()))
        }
    }

    fun onQueryChange(text: String) {
        _state.update { it.copy(search = SearchState.Open(text)) }
        typedQuery.tryEmit(text)
    }

    /** Close the field; an active query goes with it. */
    fun closeSearch() {
        _state.update { it.copy(search = SearchState.Closed) }
        typedQuery.tryEmit("")
    }

    private suspend fun applyQuery(text: String) {
        if (_state.value.feed.dataOrNull()?.query == text) return
        when (val result = backend.setQuery(text)) {
            is Outcome.Ok -> apply(result.value)
            is Outcome.Err -> failed("feed_query", result.error)
        }
    }

    // --- filter sheet --------------------------------------------------------------

    fun openFilters() {
        _state.update { it.copy(filters = FilterSheetState.Open(UiState.Loading, expanded = false)) }
        viewModelScope.launch { reloadRoster() }
    }

    fun closeFilters() {
        _state.update { it.copy(filters = FilterSheetState.Closed) }
    }

    /** "Show all N authors". */
    fun showAllAuthors() {
        _state.update { state ->
            val open = state.filters as? FilterSheetState.Open ?: return@update state
            state.copy(filters = open.copy(expanded = true))
        }
        viewModelScope.launch { reloadRoster() }
    }

    private suspend fun reloadRoster() {
        val open = _state.value.filters as? FilterSheetState.Open ?: return
        val roster = backend.authorRoster(if (open.expanded) null else ROSTER_LIMIT).logErr(TAG, "roster_failed")
        _state.update { state ->
            val current = state.filters as? FilterSheetState.Open ?: return@update state
            val keep = roster is Outcome.Err && current.roster is UiState.Loaded
            state.copy(filters = current.copy(roster = if (keep) current.roster else roster.toUiState()))
        }
        if (roster is Outcome.Err && _state.value.filters.let { it is FilterSheetState.Open && it.roster is UiState.Loaded }) {
            outbox.send(roster.error.describe())
        }
    }

    // --- account -------------------------------------------------------------------

    /** Sign out after GitHub rejected the token, so the user can sign in again. */
    fun signOut() {
        viewModelScope.launch {
            RostrumLog.i(TAG, "sign_out_from_feed")
            signOutAction()
        }
    }

    companion object {
        const val QUERY_DEBOUNCE_MILLIS = 250L
        const val ROSTER_LIMIT = 5
        private const val DEFAULT_REFRESH_SECONDS = 60L
        private const val TAG = "RostrumFeed"
    }
}
