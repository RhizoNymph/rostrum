package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.ui.common.UiState
import java.time.Instant

/** Everything the feed screen and its filter sheet render. */
data class FeedUiState(
    val feed: UiState<FeedSnapshot> = UiState.Loading,
    /** A user-started refresh is in flight (drives the pull-to-refresh spinner). */
    val refreshing: Boolean = false,
    val search: SearchState = SearchState.Closed,
    val desktop: DesktopPill = DesktopPill.Checking,
    val filters: FilterSheetState = FilterSheetState.Closed,
    /** GitHub rejected the token; the screen offers signing in again. */
    val authProblem: BackendError? = null,
    /** The instant relative ages ("2h") are measured from. */
    val now: Instant,
)

/** The search field under the header. Its text is the user's, not yet the core's. */
sealed interface SearchState {
    data object Closed : SearchState

    data class Open(val text: String) : SearchState
}

/** The filter sheet; the author roster only exists while it is open. */
sealed interface FilterSheetState {
    data object Closed : FilterSheetState

    /** [expanded]: the whole roster was asked for ("Show all N authors"). */
    data class Open(val roster: UiState<AuthorRoster>, val expanded: Boolean) : FilterSheetState
}

/** What the header's desktop pill says about the paired desktop. */
sealed interface DesktopPill {
    /** No desktop, or the desktop forgot this phone. Tap to pair. */
    data object NotPaired : DesktopPill

    /** Paired; asking the desktop who it is. */
    data object Checking : DesktopPill

    data class Connected(val name: String) : DesktopPill

    /** None of its addresses answered, or it answered too late. */
    data object Unreachable : DesktopPill

    /** It answered with something else wrong (certificate, protocol, version). */
    data class Problem(val error: BackendError) : DesktopPill
}
