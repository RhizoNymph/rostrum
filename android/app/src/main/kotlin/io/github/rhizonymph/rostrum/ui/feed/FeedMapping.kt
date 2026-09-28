package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.data.requiresPairing
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import java.time.Instant

/*
 * Pure decisions behind the feed's rows, header and filter sheet, kept out of
 * the composables so they are unit-tested.
 */

/** An icon a row chip leads with. */
enum class ChipIcon { Check, Desktop }

/** One chip under a feed row's meta line. */
data class RowChip(
    val text: String,
    val role: ColorRole,
    val mono: Boolean = false,
    val icon: ChipIcon? = null,
    val description: String? = null,
)

/**
 * The chips of a feed row, in the mockup's order: merge trouble, your review,
 * the review verdict, distance from base, then what the desktop knows.
 * Drafts are marked in the meta line instead.
 */
fun rowChips(pr: PrSummary): List<RowChip> = buildList {
    pr.mergeChip?.let { add(RowChip(it.text, it.role, description = it.tooltip)) }
    if (pr.reviewRequested) add(RowChip("Your review", ColorRole.Accent, description = "Your review is requested"))
    pr.reviewChip?.let {
        val icon = if (pr.reviewDecision == ReviewDecision.Approved) ChipIcon.Check else null
        add(RowChip(it.text, it.role, icon = icon, description = it.tooltip))
    }
    pr.behindChip?.let { add(RowChip(it.text, it.role, mono = true, description = it.tooltip)) }
    pr.localChips.forEach {
        val icon = if (it.role == ColorRole.Neutral) ChipIcon.Desktop else null
        add(RowChip(it.text, it.role, icon = icon, description = it.tooltip))
    }
}

/** `you` for your own pull requests, the author's login otherwise. */
fun authorLabel(pr: PrSummary): String = when {
    pr.isYours -> "you"
    else -> pr.author?.login ?: "ghost"
}

/** How long ago the pull request was opened: `2h`. */
fun ageLabel(pr: PrSummary, now: Instant): String = relativeAge(pr.createdAt, now)

/** How a line count is coloured: zero counts are subdued. */
enum class CountTone { Added, Removed, Zero }

fun additionsTone(count: Int): CountTone = if (count == 0) CountTone.Zero else CountTone.Added

fun deletionsTone(count: Int): CountTone = if (count == 0) CountTone.Zero else CountTone.Removed

/** The letter in a repository's tile: the name's first letter. */
fun repoInitial(repo: String): String =
    repo.substringAfter('/').firstOrNull()?.uppercaseChar()?.toString() ?: "?"

fun openCountText(count: Int): String = "$count open"

fun hiddenReposText(count: Int): String =
    "$count empty ${if (count == 1) "repository" else "repositories"} hidden"

/** Why a loaded repository shows nothing. */
fun emptyBodyText(section: RepoSection): String =
    if (section.openCount == 0) "No open pull requests" else "None match the filter"

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
fun authorSubline(isViewer: Boolean, openPrs: Int): String =
    if (isViewer) "you · $openPrs open" else "$openPrs open"

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

fun DesktopPill.view(): PillView = when (this) {
    DesktopPill.NotPaired -> PillView("Pair desktop", null, "Pair a desktop")
    DesktopPill.Checking -> PillView("Desktop", ColorRole.Neutral, "Desktop, checking")
    is DesktopPill.Connected -> PillView(name, ColorRole.Success, "Desktop $name, connected")
    DesktopPill.Unreachable -> PillView("Unreachable", ColorRole.Warning, "Desktop unreachable")
    is DesktopPill.Problem -> PillView("Desktop", ColorRole.Danger, "Desktop has a problem")
}
