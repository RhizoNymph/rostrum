package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/**
 * One render of the feed (`feed/types.rs`). Every call that changes the feed
 * returns a fresh one; so does every background update, through
 * [io.github.rhizonymph.rostrum.data.RostrumBackend.feedUpdates].
 */
data class FeedSnapshot(
    /** Increases with every change; keep the highest. */
    val revision: Long,
    /** Watched repositories in settings order, minus hidden empty ones. */
    val repos: List<RepoSection>,
    /** How many repositories `hideEmptyRepos` removed. */
    val hiddenEmptyRepos: Int,
    /** Open pull requests across every repository, before filtering. */
    val totalOpen: Int,
    /** Pull requests the filter lets through, collapsed repositories included. */
    val visibleOpen: Int,
    /** The search box. Not persisted. */
    val query: String,
    val preferences: FeedPreferences,
    /** Whether anything narrows the feed (query, drafts hidden, authors). */
    val filterActive: Boolean,
    /** GitHub is still computing merge states; a later snapshot carries them. */
    val mergeStatesSettling: Boolean,
    /** Who the GitHub token belongs to, once a refresh has said. */
    val viewer: UserRef?,
)

/** The feed filter's standing preferences, persisted in the settings file. */
data class FeedPreferences(
    val hideDrafts: Boolean,
    /** Drop repositories that loaded with nothing to show. */
    val hideEmptyRepos: Boolean,
    /** Logins the feed is narrowed to (lowercase); empty means everyone. */
    val authors: List<String>,
    /** Widen [authors] to "opened by, assigned to, or awaiting review from". */
    val includeInvolved: Boolean,
) {
    companion object {
        val Default = FeedPreferences(
            hideDrafts = false,
            hideEmptyRepos = true,
            authors = emptyList(),
            includeInvolved = false,
        )
    }
}

/** One repository's container in the feed. */
data class RepoSection(
    /** `owner/name`. */
    val repo: String,
    val load: RepoLoad,
    /** Open pull requests in this repository, before filtering. */
    val openCount: Int,
    /** Pull requests the filter lets through, whether or not collapsed. */
    val visibleCount: Int,
    val collapsed: Boolean,
    val body: RepoBody,
)

/** A repository's fetch state. */
sealed interface RepoLoad {
    /** Watched but never fetched. */
    data object Idle : RepoLoad

    data object Loading : RepoLoad

    data class Loaded(val at: Instant) : RepoLoad

    /** The last fetch failed. Older pull requests may still be shown. */
    data class Failed(val reason: String, val at: Instant) : RepoLoad
}

/** What a repository's container shows below its header. */
sealed interface RepoBody {
    data object Collapsed : RepoBody

    /** First load in flight, nothing cached. */
    data object Loading : RepoBody

    /** The fetch failed and there is nothing cached to show instead. */
    data class Failed(val reason: String) : RepoBody

    /** Loaded; no open pull requests, or none the filter lets through. */
    data object Empty : RepoBody

    data class Pulls(val pulls: List<PrSummary>) : RepoBody
}

/** Everything a feed row shows for one pull request. */
data class PrSummary(
    val repo: String,
    val number: Int,
    val title: String,
    val url: String,
    val author: UserRef?,
    val createdAt: Instant,
    val updatedAt: Instant,
    val isDraft: Boolean,
    /** CI rollup of the head commit; `null` when nothing reported. */
    val checks: CheckState?,
    val checksRole: ColorRole,
    val reviewDecision: ReviewDecision?,
    /** `approved` or `changes`; nothing while a review is merely required. */
    val reviewChip: Chip?,
    val mergeStatus: MergeStatus,
    /** `conflict`, `behind` or `blocked`. */
    val mergeChip: Chip?,
    val baseDivergence: BaseDivergence?,
    /** `↓N main` when the branch is behind its base. */
    val behindChip: Chip?,
    val labels: List<LabelView>,
    val additions: Int,
    val deletions: Int,
    val changedFiles: Int,
    val commentCount: Int,
    /** Your review is requested on it. */
    val reviewRequested: Boolean,
    /** You opened it. */
    val isYours: Boolean,
    val headRef: String,
    val baseRef: String,
) {
    val ref: PrRef get() = PrRef(repo, number)
}

/** How far a branch has drifted from its base. */
data class BaseDivergence(
    val behind: Int,
    val ahead: Int,
    val baseRef: String,
    /** Behind with no commits of its own: merge and rebase give the same result. */
    val fastForwards: Boolean,
    /** `3 commits behind main, 2 ahead`. */
    val summary: String,
)

/** The author filter's candidates, capped for display. */
data class AuthorRoster(
    /** You first, then everyone else by most recent activity. */
    val authors: List<AuthorChip>,
    /** How many the cap left out, for "Show all N authors". */
    val hidden: Int,
)

/** One person the feed can be narrowed to. */
data class AuthorChip(
    val login: String,
    val avatarUrl: String?,
    /** Open pull requests they authored across the feed. */
    val openPrs: Int,
    val isViewer: Boolean,
    val selected: Boolean,
)
