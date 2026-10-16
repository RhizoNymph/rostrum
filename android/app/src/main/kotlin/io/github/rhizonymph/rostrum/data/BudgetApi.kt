package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.ApiBudget
import io.github.rhizonymph.rostrum.data.model.PollSchedule

/**
 * The profile's GitHub rate-limit budget (`budget/mod.rs`). Budgets belong to
 * the token, so each profile has its own; a token handed over from the
 * desktop shares its budget with the desktop and `gh` there. No network.
 */
interface BudgetApi {
    /**
     * How long to wait before the next [RostrumBackend.pollFeed]: the
     * settings interval, stretched as the budget runs down, or until a
     * rate-limit pause lifts.
     */
    suspend fun pollSchedule(): Outcome<PollSchedule>

    /** The budget as of now; `null` when signed out. */
    suspend fun apiBudget(): Outcome<ApiBudget?>
}
