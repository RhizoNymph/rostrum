package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.ApiBudget
import io.github.rhizonymph.rostrum.data.model.BudgetLevel
import io.github.rhizonymph.rostrum.data.model.PauseReason
import io.github.rhizonymph.rostrum.data.model.PollPace
import io.github.rhizonymph.rostrum.data.model.PollSchedule
import io.github.rhizonymph.rostrum.data.model.RateWindow
import uniffi.rostrum_ffi.ApiBudget as FfiApiBudget
import uniffi.rostrum_ffi.ApiBudgetLevel as FfiApiBudgetLevel
import uniffi.rostrum_ffi.PauseReason as FfiPauseReason
import uniffi.rostrum_ffi.PollPace as FfiPollPace
import uniffi.rostrum_ffi.PollSchedule as FfiPollSchedule
import uniffi.rostrum_ffi.RateWindow as FfiRateWindow

/* The rate-limit budget (`budget/types.rs`): generated records → model. */

internal fun FfiApiBudgetLevel.toModel(): BudgetLevel = when (this) {
    FfiApiBudgetLevel.HEALTHY -> BudgetLevel.Healthy
    FfiApiBudgetLevel.LOW -> BudgetLevel.Low
    FfiApiBudgetLevel.CRITICAL -> BudgetLevel.Critical
    FfiApiBudgetLevel.EXHAUSTED -> BudgetLevel.Exhausted
}

internal fun FfiPauseReason.toModel(): PauseReason = when (this) {
    FfiPauseReason.PRIMARY_LIMIT -> PauseReason.PrimaryLimit
    FfiPauseReason.SECONDARY_LIMIT -> PauseReason.SecondaryLimit
}

internal fun FfiRateWindow.toModel() = RateWindow(
    remaining = remaining.toInt(),
    limit = limit.toInt(),
    resetsAt = resetsAt,
    level = level.toModel(),
)

internal fun FfiPollPace.toModel(): PollPace = when (this) {
    is FfiPollPace.Normal -> PollPace.Normal
    is FfiPollPace.Stretched -> PollPace.Stretched(level.toModel())
    is FfiPollPace.UntilReset -> PollPace.UntilReset
    is FfiPollPace.Paused -> PollPace.Paused(until, reason.toModel())
}

internal fun FfiPollSchedule.toModel() = PollSchedule(
    delaySecs = delaySecs.toLong(),
    baseSecs = baseSecs.toLong(),
    pace = pace.toModel(),
)

internal fun FfiApiBudget.toModel() = ApiBudget(
    graphql = graphql?.toModel(),
    rest = rest?.toModel(),
    schedule = schedule.toModel(),
)
