package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.StackCandidate
import io.github.rhizonymph.rostrum.data.model.StackJob
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanCheck
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.data.model.StackRewritePlan

/**
 * Stack actions, run on the paired desktop ([BackendError.NotPaired]
 * without one): each starts a [StackJob] polled with [stackJob]. The checks
 * answer locally from the cached feed; the desktop's dry run
 * ([planStackRewrite]) is authoritative. History is rewritten only for the
 * branches passed as `confirmRewrite`, exactly as the user was shown them.
 */
interface StackActionsApi {
    /** Ask the desktop which branches [request] would rewrite. */
    suspend fun planStackRewrite(request: StackPlanRequest): Outcome<StackRewritePlan>

    /** The same rules, checked on the phone. */
    suspend fun checkStackPlan(request: StackPlanRequest): Outcome<StackPlanCheck>

    /** The repository's open pull requests that could join GitHub stack [stack]. */
    suspend fun stackCandidates(repo: String, stack: Int): Outcome<List<StackCandidate>>

    /** Make a GitHub stack of a detected chain ([prs] bottom first). Nothing is rewritten. */
    suspend fun makeStack(repo: String, prs: List<Int>, trunk: String): Outcome<StackJob>

    /** Rebase [prs] into a stack on [trunk], force-pushing exactly [confirmRewrite]. */
    suspend fun arrangeStack(repo: String, prs: List<Int>, trunk: String, confirmRewrite: List<String>): Outcome<StackJob>

    /** Add [prs] on top of stack [stack], force-pushing exactly [confirmRewrite]. */
    suspend fun extendStack(repo: String, stack: Int, prs: List<Int>, confirmRewrite: List<String>): Outcome<StackJob>

    /** GitHub's all-or-nothing merge of every member. */
    suspend fun mergeStack(repo: String, stack: Int, method: StackMergeMethod): Outcome<StackJob>

    suspend fun unstack(repo: String, stack: Int): Outcome<StackJob>

    suspend fun stackJob(id: Long): Outcome<StackJob>
}
