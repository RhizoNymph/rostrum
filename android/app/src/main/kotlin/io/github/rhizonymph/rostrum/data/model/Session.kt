package io.github.rhizonymph.rostrum.data.model

/** What the backend knows about the GitHub token (`session.rs`). */
sealed interface GitHubStatus {
    /** No token has been handed in. */
    data object NoToken : GitHubStatus

    /** A token is set but has not been used successfully yet. */
    data object Unverified : GitHubStatus

    /** A request with this token succeeded; it belongs to [viewer]. */
    data class Verified(val viewer: UserRef) : GitHubStatus

    /** GitHub rejected the token. */
    data class Invalid(val reason: String) : GitHubStatus
}

/** Persisted settings (`settings.rs`). */
data class Settings(
    /** Watched repositories, `owner/name`, sorted. */
    val repos: List<String>,
    /** Seconds between foreground feed refreshes (10..=3600). */
    val refreshIntervalSecs: Long,
    /** Open pull requests fetched per repository (1..=100). */
    val prsPerRepo: Int,
    val notifyNewPullRequests: Boolean,
    val notifyReviewRequests: Boolean,
    /** Default for the "stash local changes" switch on desktop jobs. */
    val autostash: Boolean,
    val feed: FeedPreferences,
    /** Open issues fetched per repository (1..=100). */
    val issuesPerRepo: Int = 25,
)

/** Something worth a system notification (`notifications.rs`). */
data class NotificationEvent(
    val kind: NotificationKind,
    val repo: String,
    val number: Int,
    val title: String,
    val author: String?,
    val url: String,
)

enum class NotificationKind {
    /** A pull request appeared in a watched repository. */
    NewPullRequest,

    /** Your review was requested on a pull request. */
    ReviewRequested,
}
