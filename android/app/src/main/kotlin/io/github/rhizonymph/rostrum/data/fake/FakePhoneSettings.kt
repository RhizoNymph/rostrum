package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.Settings

/**
 * The phone's persisted settings in the fake, shared between
 * [FakeRostrumBackend] (the setters) and [FakeDesktopConfig] (copying and
 * pushing them).
 */
internal class FakePhoneSettings {
    val repos: MutableList<String> = SamplePulls.repos.toMutableList()
    var refreshIntervalSecs = 60L
    var prsPerRepo = 30
    var issuesPerRepo = 25
    var notifyNew = true
    var notifyReviews = true
    var autostash = false
    var preferences = FeedPreferences(
        hideDrafts = false,
        hideEmptyRepos = true,
        authors = listOf("rhizonymph", "ada-lin"),
        includeInvolved = true,
    )

    fun toSettings() = Settings(
        repos = repos.toList(),
        refreshIntervalSecs = refreshIntervalSecs,
        prsPerRepo = prsPerRepo,
        notifyNewPullRequests = notifyNew,
        notifyReviewRequests = notifyReviews,
        autostash = autostash,
        feed = preferences,
        issuesPerRepo = issuesPerRepo,
    )
}
