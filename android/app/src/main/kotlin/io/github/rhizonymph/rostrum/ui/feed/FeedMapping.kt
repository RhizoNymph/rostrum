package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.data.requiresPairing

/*
 * Pure decisions behind the feed's rows, header and filter sheet, kept out of
 * the composables so they are unit-tested.
 */

fun openCountText(count: Int): String = "$count open"

fun hiddenReposText(count: Int): String =
    "$count empty ${if (count == 1) "repository" else "repositories"} hidden"

/** Why a loaded repository shows nothing, in the words of its tab. */
fun emptyBodyText(section: RepoSection, tab: FeedTab = FeedTab.PullRequests): String = when {
    section.openCount > 0 -> "None match the filter"
    tab == FeedTab.Issues -> "No open issues"
    else -> "No open pull requests"
}

/**
 * How many of the sheet's saved settings narrow the feed. Involvement only
 * counts when there are authors for it to widen; hiding empty repositories
 * never hides a pull request.
 */
fun activeFilterCount(preferences: FeedPreferences): Int {
    var count = 0
    if (preferences.authors.isNotEmpty()) count++
    if (preferences.authors.isNotEmpty() && preferences.includeInvolved) count++
    if (preferences.hideDrafts) count++
    return count
}

/** `2 active`, or nothing when no filter narrows the feed. */
fun activeText(count: Int): String? = if (count == 0) null else "$count active"

fun filterButtonDescription(active: Int): String =
    if (active == 0) "Feed filters" else "Feed filters, $active active"

/** `Authors: me, ada-lin`, `Authors: me, ada-lin +2`, or `Authors` when none are chosen. */
fun authorsChipLabel(preferences: FeedPreferences, viewerLogin: String?): String {
    val authors = preferences.authors
    if (authors.isEmpty()) return "Authors"
    val names = authors.map { if (viewerLogin != null && it.equals(viewerLogin, ignoreCase = true)) "me" else it }
    val shown = names.take(2).joinToString(", ")
    val rest = names.size - 2
    return if (rest > 0) "Authors: $shown +$rest" else "Authors: $shown"
}

/** Under an author's name in the sheet: `you · 2 open`. */
fun authorSubline(isViewer: Boolean, openItems: Int): String =
    if (isViewer) "you · $openItems open" else "$openItems open"

/**
 * The desktop pill from pairing and the desktop's answer to `machineInfo`
 * (`null` while the question is in flight).
 */
fun desktopPillOf(paired: Boolean, info: Outcome<MachineInfo>?): DesktopPill = when {
    !paired -> DesktopPill.NotPaired
    info == null -> DesktopPill.Checking
    info is Outcome.Ok -> DesktopPill.Connected(info.value.name)
    else -> when (val error = (info as Outcome.Err).error) {
        BackendError.DesktopTimeout, is BackendError.DesktopUnreachable -> DesktopPill.Unreachable
        else -> if (error.requiresPairing) DesktopPill.NotPaired else DesktopPill.Problem(error)
    }
}

/** The pill as drawn: label, dot colour (none for "Pair desktop"), accessibility text. */
data class PillView(val label: String, val dot: ColorRole?, val description: String)

/**
 * The header pill: the active profile's name (tap to switch profiles), with
 * a dot for its desktop's state, none when it has no desktop.
 */
fun DesktopPill.view(profile: String): PillView = when (this) {
    DesktopPill.NotPaired -> PillView(profile, null, "Profile $profile. Switch profiles")
    DesktopPill.Checking -> PillView(profile, ColorRole.Neutral, "Profile $profile, checking the desktop. Switch profiles")
    is DesktopPill.Connected -> PillView(profile, ColorRole.Success, "Profile $profile, desktop $name connected. Switch profiles")
    DesktopPill.Unreachable -> PillView(profile, ColorRole.Warning, "Profile $profile, desktop unreachable. Switch profiles")
    is DesktopPill.Problem -> PillView(profile, ColorRole.Danger, "Profile $profile, the desktop has a problem. Switch profiles")
}
