package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/**
 * A profile's GitHub rate-limit budget (`budget/types.rs`). Budgets belong to
 * the token, so this one is shared with anything else using it — the paired
 * desktop and `gh` there, when the token was handed over from it.
 */
data class ApiBudget(
    /** GraphQL points; `null` until a response has reported them. */
    val graphql: RateWindow?,
    /** REST requests; `null` until a response has reported them. */
    val rest: RateWindow?,
    /** The next background refresh, including any pause in force. */
    val schedule: PollSchedule,
) {
    /** Until when GitHub has paused polling, or `null`. */
    val pausedUntil: Instant? get() = (schedule.pace as? PollPace.Paused)?.until
}

/** One hourly budget, as of when it was read. */
data class RateWindow(
    /** Left in this window; the whole limit once the window has reset. */
    val remaining: Int,
    /** At least 1. */
    val limit: Int,
    /** When the window refills; `null` once it has. */
    val resetsAt: Instant?,
    val level: BudgetLevel,
)

/** How much is left, by the desktop's thresholds (20% and 10%). */
enum class BudgetLevel { Healthy, Low, Critical, Exhausted }

/** Which limit GitHub refused for. */
enum class PauseReason {
    /** The hourly budget is spent; it refills at the reset. */
    PrimaryLimit,

    /** Too many requests too quickly. */
    SecondaryLimit,
}

/** Why the next background refresh waits as long as it does. */
sealed interface PollPace {
    /** The configured interval. */
    data object Normal : PollPace

    /** Stretched because the GraphQL budget is [level] (never healthy). */
    data class Stretched(val level: BudgetLevel) : PollPace

    /** Nothing usable is left before the reset: wait for it. */
    data object UntilReset : PollPace

    /** GitHub refused for rate limiting: nothing is sent until [until]. */
    data class Paused(val until: Instant, val reason: PauseReason) : PollPace
}

/** When the next background refresh should run. */
data class PollSchedule(
    /** Seconds until the next refresh; at least 1. */
    val delaySecs: Long,
    /** The configured interval. */
    val baseSecs: Long,
    val pace: PollPace,
)
