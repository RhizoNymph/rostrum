package io.github.rhizonymph.rostrum.ui.navigation

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
     * A tapped notification about a pull request, in [profile] (`null` when
     * the intent named none: the active profile).
     */
    data class OpenPullRequest(val pr: PrRef, val profile: ProfileId? = null) : AppLink
}

object AppLinks {
    const val EXTRA_REPO = "io.github.rhizonymph.rostrum.extra.REPO"
    const val EXTRA_NUMBER = "io.github.rhizonymph.rostrum.extra.NUMBER"
    const val EXTRA_PROFILE = "io.github.rhizonymph.rostrum.extra.PROFILE"
    private const val ACTION_VIEW = "android.intent.action.VIEW"

    /**
     * Read an intent's parts (kept as plain values so this runs on the JVM):
     * a VIEW of `rostrum://pair…`, or the extras a notification sets. A
     * malformed profile id is ignored (the pull request opens in the active
     * profile).
     */
    fun parse(action: String?, data: String?, repo: String?, number: Int?, profile: String? = null): AppLink? {
        if (action == ACTION_VIEW && data != null && isPairLink(data)) return AppLink.Pair(data)
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
    is AppLink.Pair -> LinkRoute.Show
    is AppLink.OpenPullRequest -> when (val target = link.profile) {
        null, profiles.active -> LinkRoute.Show
        else -> if (profiles.profile(target) != null) LinkRoute.SwitchFirst(target) else LinkRoute.ProfileGone
    }
}
