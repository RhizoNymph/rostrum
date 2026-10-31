package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.model.ApiBudget
import io.github.rhizonymph.rostrum.data.model.BudgetLevel
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.PauseReason
import io.github.rhizonymph.rostrum.data.model.PollPace
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.model.RateWindow
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/*
 * The feed's GitHub budget chip and its explanation, kept pure so the wording
 * and colours are unit-tested. The thresholds are the core's (the desktop's):
 * quiet at 20% or more, warning below, danger below 10%, at 0, or paused.
 */

/** Who else spends the profile's token's budget: budgets belong to the token. */
sealed interface BudgetSharing {
    /** The token came from the desktop [machine] (handed over at pairing). */
    data class Desktop(val machine: String) : BudgetSharing

    /** A token pasted on the phone. */
    data object PastedToken : BudgetSharing

    companion object {
        /** A desktop profile's token is the desktop's (`gh`'s), handed over when paired. */
        fun of(kind: ProfileKind?): BudgetSharing = when (kind) {
            is ProfileKind.Desktop -> Desktop(kind.machine)
            ProfileKind.TokenOnly, null -> PastedToken
        }
    }
}

/** The chip as drawn: its text, colour role, and what a screen reader says. */
data class BudgetChipView(val text: String, val role: ColorRole, val description: String)

/** What the explanation sheet says, paragraph by paragraph. */
data class BudgetExplanation(val title: String, val lines: List<String>)

private fun clock(at: Instant, zone: ZoneId): String = DateTimeFormatter.ofPattern("HH:mm").withZone(zone).format(at)

/** The pause in force at [now], if any. */
private fun ApiBudget.pauseAt(now: Instant): PollPace.Paused? =
    (schedule.pace as? PollPace.Paused)?.takeIf { it.until.isAfter(now) }

/** A window as of [now]: once its reset has passed it is full again. */
private fun RateWindow.at(now: Instant): RateWindow =
    if (resetsAt != null && !resetsAt.isAfter(now)) copy(remaining = limit, resetsAt = null, level = BudgetLevel.Healthy) else this

fun BudgetLevel.role(): ColorRole = when (this) {
    BudgetLevel.Healthy -> ColorRole.Neutral
    BudgetLevel.Low -> ColorRole.Warning
    BudgetLevel.Critical, BudgetLevel.Exhausted -> ColorRole.Danger
}

/**
 * `API 4123/5000 · resets 14:05`, `API 5000/5000` once the window has reset,
 * or `paused until 14:32`; `null` before GitHub has reported anything (or
 * signed out).
 */
fun budgetChip(budget: ApiBudget?, now: Instant, zone: ZoneId): BudgetChipView? {
    if (budget == null) return null
    budget.pauseAt(now)?.let { pause ->
        val until = clock(pause.until, zone)
        return BudgetChipView("paused until $until", ColorRole.Danger, "GitHub paused refreshing until $until. Details")
    }
    val window = budget.graphql?.at(now) ?: return null
    val resets = window.resetsAt
    return if (resets == null) {
        BudgetChipView(
            "API ${window.remaining}/${window.limit}",
            window.level.role(),
            "GitHub API budget full, ${window.remaining} of ${window.limit}. Details",
        )
    } else {
        val at = clock(resets, zone)
        BudgetChipView(
            "API ${window.remaining}/${window.limit} · resets $at",
            window.level.role(),
            "GitHub API budget, ${window.remaining} of ${window.limit} left, resets at $at. Details",
        )
    }
}

/** `1 min`, `4 min`, `90 s`. */
fun intervalText(seconds: Long): String =
    if (seconds >= 60 && seconds % 60 == 0L) "${seconds / 60} min" else "$seconds s"

private fun windowLine(name: String, unit: String, window: RateWindow?, now: Instant, zone: ZoneId): String {
    val current = window?.at(now) ?: return "$name: not reported yet."
    val resets = current.resetsAt
    return if (resets == null) {
        "$name: ${current.limit} $unit, full again (the hourly window has reset)."
    } else {
        "$name: ${current.remaining} of ${current.limit} $unit left, resets at ${clock(resets, zone)}."
    }
}

private fun sharingLine(sharing: BudgetSharing): String = when (sharing) {
    is BudgetSharing.Desktop ->
        "This profile's token was handed over from ${sharing.machine}, so this budget is shared with rostrum " +
            "and gh there: whatever they spend, the phone has less, and the other way round."
    BudgetSharing.PastedToken ->
        "GitHub budgets belong to the token: anything else using this token (gh, scripts, another rostrum) " +
            "spends from the same budget."
}

private fun paceLine(budget: ApiBudget, now: Instant, zone: ZoneId): String {
    budget.pauseAt(now)?.let { pause ->
        val until = clock(pause.until, zone)
        val why = when (pause.reason) {
            PauseReason.PrimaryLimit -> "GitHub refused requests: the hourly budget is spent."
            PauseReason.SecondaryLimit -> "GitHub refused requests for coming too fast (its secondary rate limit)."
        }
        return "$why Nothing is sent until $until, background notification checks included."
    }
    val schedule = budget.schedule
    return when (val pace = schedule.pace) {
        PollPace.Normal, is PollPace.Paused -> "Refreshing every ${intervalText(schedule.baseSecs)} while the feed is open."
        is PollPace.Stretched ->
            "Refreshing slowed to every ${intervalText(schedule.delaySecs)} (from ${intervalText(schedule.baseSecs)}) " +
                "so the budget lasts until the reset${if (pace.level == BudgetLevel.Low) "" else " — under 10% is left"}."
        PollPace.UntilReset -> "Too little is left for a refresh: the next one waits for the reset."
    }
}

/** The sheet the chip opens: who shares the budget, what is left of each, and how refreshing adapts. */
fun budgetExplanation(budget: ApiBudget, sharing: BudgetSharing, now: Instant, zone: ZoneId): BudgetExplanation =
    BudgetExplanation(
        title = "GitHub API budget",
        lines = listOf(
            sharingLine(sharing),
            windowLine("GraphQL", "points", budget.graphql, now, zone),
            windowLine("REST", "requests", budget.rest, now, zone),
            paceLine(budget, now, zone),
            "Below 20% left, refreshing slows down so the rest lasts until the reset; " +
                "pull to refresh always asks at once unless GitHub has paused requests.",
        ),
    )
