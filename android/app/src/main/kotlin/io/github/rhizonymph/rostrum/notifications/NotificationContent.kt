package io.github.rhizonymph.rostrum.notifications

import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId

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
    /** Stable per profile and pull request, so a second event replaces the first. */
    val id: Int,
    /** Starts with the profile's label, so you know which desktop (or account) it is about. */
    val title: String,
    val text: String,
    val pr: PrRef,
    /** Tapping it switches to this profile before opening [pr]. */
    val profile: ProfileId,
)

object NotificationContent {
    fun of(event: NotificationEvent, profile: Profile): NotificationSpec {
        val pr = PrRef(event.repo, event.number)
        val author = event.author ?: "Someone"
        val (channel, title, text) = when (event.kind) {
            NotificationKind.ReviewRequested -> Triple(
                RostrumChannel.ReviewRequests,
                "$author asked for your review",
                "${event.repo} #${event.number} · ${event.title}",
            )
            NotificationKind.NewPullRequest -> Triple(
                RostrumChannel.NewPullRequests,
                "New pull request in ${event.repo}",
                "#${event.number} ${event.title} · $author",
            )
        }
        return NotificationSpec(
            channel = channel,
            id = idOf(profile.id, pr),
            title = "${profile.label} · $title",
            text = text,
            pr = pr,
            profile = profile.id,
        )
    }

    fun idOf(profile: ProfileId, pr: PrRef): Int = "$profile:${pr.repo.lowercase()}#${pr.number}".hashCode()

    /** The extras the tap intent carries; [io.github.rhizonymph.rostrum.ui.navigation.AppLinks.parse] reads them back. */
    fun tapExtras(spec: NotificationSpec): NotificationTap =
        NotificationTap(repo = spec.pr.repo, number = spec.pr.number, profile = spec.profile.value)
}

/** A notification tap's intent extras, as plain values (see [NotificationPoster]). */
data class NotificationTap(val repo: String, val number: Int, val profile: String)
