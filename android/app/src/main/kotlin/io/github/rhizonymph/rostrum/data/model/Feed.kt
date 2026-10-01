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
    /** Which list the feed shows; everything below but [tabCounts] is for this tab. */
    val tab: FeedTab,
    /** Visible items per tab, for the tab bar. */
    val tabCounts: TabCounts,
    /** The saved repository and item sorts; the core already ordered [repos] by them. */
    val sort: SortSettings,
    /** Watched repositories in the repository sort, minus hidden empty ones. */
    val repos: List<RepoSection>,
    /** How many repositories `hideEmptyRepos` removed. */
    val hiddenEmptyRepos: Int,
    /** Open items of the tab across every repository, before filtering. */
    val totalOpen: Int,
    /** Items of the tab the filter lets through, collapsed repositories included. */
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

/** The feed's two lists. */
enum class FeedTab { PullRequests, Issues }

/** Items the filter lets through, per tab. */
data class TabCounts(val pullRequests: Int, val issues: Int) {
    fun of(tab: FeedTab): Int = when (tab) {
        FeedTab.PullRequests -> pullRequests
        FeedTab.Issues -> issues
    }
}

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
    /** Open items of the tab in this repository, before filtering. */
    val openCount: Int,
    /** Items the filter lets through, whether or not collapsed. */
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

    /** Loaded; no open items, or none the filter lets through. */
    data object Empty : RepoBody

    /** The Pull requests tab: single pull requests and stacks, in the item sort. */
    data class Pulls(val items: List<PullItem>) : RepoBody {
        /** Every pull request listed, stack members included, in display order. */
        val pulls: List<PrSummary> get() = items.flatMap { it.pulls }
    }

    /** The Issues tab, in the item sort. */
    data class Issues(val issues: List<IssueSummary>) : RepoBody
}

/** One entry of a pull request list: a pull request, or a stack sorted as one unit. */
sealed interface PullItem {
    /** For list keys. */
    val key: String

    /** The pull requests this item shows, in display order. */
    val pulls: List<PrSummary>

    data class Single(val pull: PrSummary) : PullItem {
        override val pulls: List<PrSummary> get() = listOf(pull)

        override val key: String get() = "pr:${pull.repo}#${pull.number}"
    }

    /** A stack's header and its visible members, bottom first. */
    data class Stack(val stack: StackSummary, val members: List<PrSummary>) : PullItem {
        override val pulls: List<PrSummary> get() = members

        override val key: String
            get() = "stack:${members.firstOrNull()?.let { "${it.repo}#${it.number}" } ?: stack.title}"
    }
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
    /** Open items of the active tab they authored across the feed. */
    val openItems: Int,
    val isViewer: Boolean,
    val selected: Boolean,
)
