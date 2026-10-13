package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/** Something outside the app asked it to show a particular screen. */
sealed interface AppLink {
    /** A `rostrum://pair?…` link from the desktop's page or its QR code. */
    data class Pair(val uri: String) : AppLink

    /**
     * A pull request to open: a tapped notification, in [profile], or a
     * `rostrum://pr?…` link from another app (`null` profile: the active
     * one).
     */
    data class OpenPullRequest(val pr: PrRef, val profile: ProfileId? = null) : AppLink

    /** A `rostrum://issue?…` link from another app: one issue, in the active profile. */
    data class OpenIssue(val issue: IssueRef) : AppLink

    /** A `rostrum://pr` or `rostrum://issue` link that does not parse; [reason] says why. */
    data class Invalid(val reason: String) : AppLink
}

object AppLinks {
    const val EXTRA_REPO = "io.github.rhizonymph.rostrum.extra.REPO"
    const val EXTRA_NUMBER = "io.github.rhizonymph.rostrum.extra.NUMBER"
    const val EXTRA_PROFILE = "io.github.rhizonymph.rostrum.extra.PROFILE"
    private const val ACTION_VIEW = "android.intent.action.VIEW"

    /**
     * Read an intent's parts (kept as plain values so this runs on the JVM):
     * a VIEW of `rostrum://pair…`, `rostrum://pr…` or `rostrum://issue…`
     * ([ItemLinks]), or the extras a notification sets. A malformed profile
     * id is ignored (the pull request opens in the active profile).
     */
    fun parse(action: String?, data: String?, repo: String?, number: Int?, profile: String? = null): AppLink? {
        if (action == ACTION_VIEW && data != null && isPairLink(data)) return AppLink.Pair(data)
        if (action == ACTION_VIEW && data != null && ItemLinks.isItemLink(data)) {
            return when (val item = ItemLinks.parse(data)) {
                is ItemLink.Pull -> AppLink.OpenPullRequest(item.pr)
                is ItemLink.Issue -> AppLink.OpenIssue(item.issue)
                is ItemLink.Invalid -> AppLink.Invalid(item.reason)
            }
        }
        if (repo != null && number != null && number > 0 && '/' in repo) {
            return AppLink.OpenPullRequest(PrRef(repo, number), profile?.let(ProfileId::of))
        }
        return null
    }

    fun isPairLink(uri: String): Boolean {
        val lower = uri.trim().lowercase()
        return lower.startsWith("rostrum://pair?") || lower == "rostrum://pair" || lower.startsWith("rostrum://pair/")
    }
}

/**
 * Holds the latest link until the navigation host takes it. A link that
 * arrives before the UI is ready (cold start) waits here; a newer link
 * replaces an unconsumed older one.
 */
class AppLinkInbox {
    private val _pending = MutableStateFlow<AppLink?>(null)
    val pending: StateFlow<AppLink?> = _pending.asStateFlow()

    fun offer(link: AppLink) {
        _pending.value = link
    }

    /** Take [link] if it is still the pending one. */
    fun consume(link: AppLink) {
        _pending.update { current -> if (current == link) null else current }
    }
}

/** What the root does with a pending link, given the profiles. */
sealed interface LinkRoute {
    /** The graph on screen handles it (pairing, or a pull request in the active profile). */
    data object Show : LinkRoute

    /** Switch to [profile] first; its graph then opens the pull request. */
    data class SwitchFirst(val profile: ProfileId) : LinkRoute

    /** The notification's profile was removed since; drop the link. */
    data object ProfileGone : LinkRoute
}

fun routeOf(link: AppLink, profiles: ProfilesState.Ready): LinkRoute = when (link) {
    is AppLink.Pair, is AppLink.OpenIssue, is AppLink.Invalid -> LinkRoute.Show
    is AppLink.OpenPullRequest -> when (val target = link.profile) {
        null, profiles.active -> LinkRoute.Show
        else -> if (profiles.profile(target) != null) LinkRoute.SwitchFirst(target) else LinkRoute.ProfileGone
    }
}

/** What a profile's navigation graph does with the pending link. */
sealed interface LinkStep {
    /** Go to [destination]; the link is done. */
    data class Navigate(val destination: Destination) : LinkStep

    /** Say [message] instead; the link is done. */
    data class Say(val message: String) : LinkStep

    /** Another profile's notification: leave it for the root, which switches profiles first. */
    data object Wait : LinkStep
}

/**
 * The graph's half of handling [link], given whether this profile is signed
 * in and which profile it is ([profile], `null` before any exists). A pull
 * request or issue opens on its own screen, which fetches it whether or not
 * the feed holds it; a pull request outside the feed is looked up on GitHub
 * by the core. Without a GitHub token nothing can load, so the link is
 * answered with a sentence instead.
 */
fun stepFor(link: AppLink, signedIn: Boolean, profile: ProfileId?): LinkStep = when (link) {
    is AppLink.Pair -> LinkStep.Navigate(Destination.Pair(link.uri))
    is AppLink.OpenPullRequest -> when {
        link.profile != null && link.profile != profile -> LinkStep.Wait
        signedIn -> LinkStep.Navigate(Destination.PullRequest(link.pr.repo, link.pr.number))
        else -> LinkStep.Say(signInFirst("${link.pr.repo}#${link.pr.number}"))
    }
    is AppLink.OpenIssue ->
        if (signedIn) {
            LinkStep.Navigate(Destination.Issue(link.issue.repo, link.issue.number))
        } else {
            LinkStep.Say(signInFirst("${link.issue.repo}#${link.issue.number}"))
        }
    is AppLink.Invalid -> LinkStep.Say("Rostrum can't open this link: ${link.reason}")
}

private fun signInFirst(item: String) = "Sign in to GitHub first, then open the link to $item again."
