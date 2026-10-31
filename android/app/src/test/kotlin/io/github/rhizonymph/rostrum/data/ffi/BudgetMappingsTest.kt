package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.ApiBudget
import io.github.rhizonymph.rostrum.data.model.BudgetLevel
import io.github.rhizonymph.rostrum.data.model.PauseReason
import io.github.rhizonymph.rostrum.data.model.PollPace
import io.github.rhizonymph.rostrum.data.model.PollSchedule
import io.github.rhizonymph.rostrum.data.model.RateWindow
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import java.time.Instant
import uniffi.rostrum_ffi.ApiBudget as FApiBudget
import uniffi.rostrum_ffi.ApiBudgetLevel as FApiBudgetLevel
import uniffi.rostrum_ffi.PauseReason as FPauseReason
import uniffi.rostrum_ffi.PollPace as FPollPace
import uniffi.rostrum_ffi.PollSchedule as FPollSchedule
import uniffi.rostrum_ffi.RateWindow as FRateWindow

class BudgetMappingsTest {
    private val resets = Instant.parse("2026-09-28T14:05:00Z")

    @Test
    fun `a budget maps window by window`() {
        val ffi = FApiBudget(
            graphql = FRateWindow(remaining = 4123u, limit = 5000u, resetsAt = resets, level = FApiBudgetLevel.HEALTHY),
            rest = FRateWindow(remaining = 5000u, limit = 5000u, resetsAt = null, level = FApiBudgetLevel.HEALTHY),
            schedule = FPollSchedule(delaySecs = 60u, baseSecs = 60u, pace = FPollPace.Normal),
        )
        assertEquals(
            ApiBudget(
                graphql = RateWindow(4123, 5000, resets, BudgetLevel.Healthy),
                rest = RateWindow(5000, 5000, null, BudgetLevel.Healthy),
                schedule = PollSchedule(60, 60, PollPace.Normal),
            ),
            ffi.toModel(),
        )
    }

    @Test
    fun `every level maps`() {
        assertEquals(
            listOf(BudgetLevel.Healthy, BudgetLevel.Low, BudgetLevel.Critical, BudgetLevel.Exhausted),
            FApiBudgetLevel.entries.map { it.toModel() },
        )
    }

    @Test
    fun `every pace maps, a pause with its time and reason`() {
        assertEquals(PollPace.Normal, FPollPace.Normal.toModel())
        assertEquals(PollPace.Stretched(BudgetLevel.Critical), FPollPace.Stretched(FApiBudgetLevel.CRITICAL).toModel())
        assertEquals(PollPace.UntilReset, FPollPace.UntilReset.toModel())
        assertEquals(
            PollPace.Paused(resets, PauseReason.PrimaryLimit),
            FPollPace.Paused(resets, FPauseReason.PRIMARY_LIMIT).toModel(),
        )
        assertEquals(PauseReason.SecondaryLimit, FPauseReason.SECONDARY_LIMIT.toModel())
    }

    @Test
    fun `a paused schedule exposes until when`() {
        val budget = FApiBudget(
            graphql = null,
            rest = null,
            schedule = FPollSchedule(90u, 60u, FPollPace.Paused(resets, FPauseReason.SECONDARY_LIMIT)),
        ).toModel()
        assertEquals(resets, budget.pausedUntil)
        assertEquals(90L, budget.schedule.delaySecs)
    }
}
