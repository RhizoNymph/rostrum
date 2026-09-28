package io.github.rhizonymph.rostrum.notifications

import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PrRef

/** The two channels the user can silence separately in system settings. */
enum class RostrumChannel(val id: String, val title: String, val description: String) {
    NewPullRequests(
        "new_pull_requests",
        "New pull requests",
        "A pull request was opened in a repository you watch",
    ),
    ReviewRequests(
        "review_requests",
        "Review requests",
        "Someone asked for your review",
    ),
}

/** Everything one posted notification needs, decided without Android types. */
data class NotificationSpec(
    val channel: RostrumChannel,
    /** Stable per pull request, so a second event replaces the first. */
    val id: Int,
    val title: String,
    val text: String,
    val pr: PrRef,
)

object NotificationContent {
    fun of(event: NotificationEvent): NotificationSpec {
        val pr = PrRef(event.repo, event.number)
        val author = event.author ?: "Someone"
        return when (event.kind) {
            NotificationKind.ReviewRequested -> NotificationSpec(
                channel = RostrumChannel.ReviewRequests,
                id = idOf(pr),
                title = "$author asked for your review",
                text = "${event.repo} #${event.number} · ${event.title}",
                pr = pr,
            )
            NotificationKind.NewPullRequest -> NotificationSpec(
                channel = RostrumChannel.NewPullRequests,
                id = idOf(pr),
                title = "New pull request in ${event.repo}",
                text = "#${event.number} ${event.title} · $author",
                pr = pr,
            )
        }
    }

    fun idOf(pr: PrRef): Int = "${pr.repo.lowercase()}#${pr.number}".hashCode()
}
