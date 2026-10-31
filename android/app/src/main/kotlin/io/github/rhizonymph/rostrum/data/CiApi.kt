package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiCheckOutput
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiTrigger

/**
 * The CI grid (`ci/mod.rs`): every open pull request's checks in the feed's
 * order, job logs, another app's check output, and re-runs. Checks are only
 * fetched by [refreshCi] and [refreshCiRepo]; [ciGrid] rebuilds from what is
 * held, so it is cheap enough to call once a second.
 */
interface CiApi {
    /** The grid from the checks held, as of now. No network. */
    suspend fun ciGrid(filter: CiGridFilter): Outcome<CiGrid>

    /**
     * Fetch the checks [trigger] calls for across every watched repository,
     * then the grid. A failing repository says so in its [CiGrid.repos]
     * entry. While GitHub has paused polling nothing is sent; an
     * [CiTrigger.Explicit] fetch then fails with [BackendError.GitHubRateLimited].
     */
    suspend fun refreshCi(filter: CiGridFilter, trigger: CiTrigger = CiTrigger.Explicit): Outcome<CiGrid>

    /** [refreshCi] for one watched repository. */
    suspend fun refreshCiRepo(repo: String, filter: CiGridFilter, trigger: CiTrigger = CiTrigger.Explicit): Outcome<CiGrid>

    /** An Actions job's log, parsed; the last 20 000 lines unless [full]. */
    suspend fun jobLog(repo: String, jobId: Long, full: Boolean): Outcome<CiJobLog>

    /** Another app's check run: its output and annotations. */
    suspend fun checkOutput(repo: String, checkRunId: Long): Outcome<CiCheckOutput>

    /** The re-runs the check in column [key] of pull request [pr] offers. */
    suspend fun rerunTargets(repo: String, pr: Int, key: CiCheckKey): Outcome<CiRerunChoice>

    /**
     * Ask GitHub to re-run [target]; the checks it covers show as queued at
     * once. Refusals: [BackendError.CiNoPermission], [BackendError.CiNotRerunnable],
     * [BackendError.CiNotFound].
     */
    suspend fun rerun(repo: String, target: CiRerun): Outcome<Unit>
}
