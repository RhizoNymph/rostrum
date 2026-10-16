package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.BudgetApi
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.ApiBudget
import io.github.rhizonymph.rostrum.data.model.BudgetLevel
import io.github.rhizonymph.rostrum.data.model.PauseReason
import io.github.rhizonymph.rostrum.data.model.PollPace
import io.github.rhizonymph.rostrum.data.model.PollSchedule
import io.github.rhizonymph.rostrum.data.model.RateWindow
import java.time.Clock
import java.time.Duration
import java.time.Instant

/**
 * The sample token's rate-limit budget. Tests set [graphql], [rest] and
 * [pausedUntil]; while a pause holds, the fake's background fetches send
 * nothing and explicit ones fail with [BackendError.GitHubRateLimited], as
 * the core does. Pacing follows the core's floors: twice the interval when
 * low, four times when critical or spent.
 */
internal class FakeBudget(
    private val host: FakeHost,
    private val clock: Clock,
    started: Instant,
    private val phone: FakePhoneSettings,
) : BudgetApi {
    var graphql: RateWindow? = RateWindow(4123, 5000, started.plus(Duration.ofMinutes(37)), BudgetLevel.Healthy)
    var rest: RateWindow? = RateWindow(4987, 5000, started.plus(Duration.ofMinutes(52)), BudgetLevel.Healthy)
    var pausedUntil: Instant? = null
    var pauseReason: PauseReason = PauseReason.SecondaryLimit

    /** GitHub has paused polling, as of now. */
    val pause: PollPace.Paused?
        get() = pausedUntil?.takeIf { it.isAfter(clock.instant()) }?.let { PollPace.Paused(it, pauseReason) }

    /** The error an explicit request answers with while paused, or `null`. */
    fun refusal(): BackendError.GitHubRateLimited? = pause?.let { BackendError.GitHubRateLimited(it.until) }

    fun schedule(): PollSchedule {
        val base = phone.refreshIntervalSecs
        pause?.let { paused ->
            val left = Duration.between(clock.instant(), paused.until).seconds.coerceAtLeast(1)
            return PollSchedule(left, base, paused)
        }
        return when (val level = graphql?.level) {
            BudgetLevel.Low -> PollSchedule(base * 2, base, PollPace.Stretched(level))
            BudgetLevel.Critical, BudgetLevel.Exhausted -> PollSchedule(base * 4, base, PollPace.Stretched(level))
            BudgetLevel.Healthy, null -> PollSchedule(base, base, PollPace.Normal)
        }
    }

    fun budget(): ApiBudget = ApiBudget(graphql, rest, schedule())

    override suspend fun pollSchedule(): Outcome<PollSchedule> = host.call(FakeCall.PollSchedule) { Outcome.Ok(schedule()) }

    override suspend fun apiBudget(): Outcome<ApiBudget?> = host.call(FakeCall.ApiBudget) {
        when (val signedIn = host.signedIn { Outcome.Ok(budget()) }) {
            is Outcome.Ok -> signedIn
            is Outcome.Err -> Outcome.Ok(null)
        }
    }
}
