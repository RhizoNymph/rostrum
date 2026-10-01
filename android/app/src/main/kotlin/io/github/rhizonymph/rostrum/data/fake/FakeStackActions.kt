package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.StackActionsApi
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.StackCandidate
import io.github.rhizonymph.rostrum.data.model.StackEligibility
import io.github.rhizonymph.rostrum.data.model.StackJob
import io.github.rhizonymph.rostrum.data.model.StackJobKind
import io.github.rhizonymph.rostrum.data.model.StackJobResult
import io.github.rhizonymph.rostrum.data.model.StackJobState
import io.github.rhizonymph.rostrum.data.model.StackKind
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanCheck
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.data.model.StackRewrite
import io.github.rhizonymph.rostrum.data.model.StackRewritePlan
import java.time.Clock

/**
 * The fake's stack actions, as the core and the desktop answer them: the
 * same input checks, plans from base/head chains (a member whose base is
 * not the head below it is rewritten), a refusal unless exactly the planned
 * branches are confirmed, and jobs that finish on their [POLLS_TO_FINISH]th
 * poll with [nextOutcome] (a plain success unless a test sets one). Nothing
 * in the sample data changes when a job finishes; the feed is republished.
 */
internal class FakeStackActions(private val host: FakeHost, private val clock: Clock) : StackActionsApi {
    private class Running(val job: StackJob, val result: StackJobResult, var polls: Int = 0)

    private val jobs = linkedMapOf<Long, Running>()
    private var nextId = 1L

    /** How the next job to finish ends; `null` is success. Cleared once used. */
    var nextOutcome: StackJobState? = null

    private fun <T> desktop(block: () -> Outcome<T>): Outcome<T> =
        host.signedIn { if (!host.isPaired) Outcome.Err(BackendError.NotPaired) else block() }

    private fun invalid(reason: String) = Outcome.Err(BackendError.InvalidInput(reason))

    private fun githubStack(repo: String, stack: Int): FakeStackDef? =
        FakeStacks.samples.firstOrNull { it.repo == repo && (it.kind as? StackKind.GitHub)?.number == stack }

    /** The input checks the core makes before asking the desktop. */
    private fun checkInput(prs: List<Int>, minimum: Int, trunk: String? = null, stack: Int? = null): String? = when {
        prs.size != prs.toSet().size -> "List each pull request once"
        prs.size < minimum -> if (minimum == 1) "Pick a pull request to add" else "A stack needs at least $minimum pull requests"
        trunk != null && !FakeRepoView.validBranchName(trunk) -> "$trunk isn't a branch name"
        stack != null && stack <= 0 -> "Stack numbers start at 1"
        else -> null
    }

    /** The plan for [request]: which of its pull requests would be rewritten, or why it can't run. */
    private fun plan(request: StackPlanRequest): StackPlanCheck {
        val open = host.openPulls(request.repo).associateBy { it.number }
        request.prs.firstOrNull { it !in open }?.let { return StackPlanCheck.Invalid("#$it isn't an open pull request in ${request.repo}") }
        val (firstBase, minimum) = when (request) {
            is StackPlanRequest.Arrange -> request.trunk to 2
            is StackPlanRequest.Extend -> {
                val def = githubStack(request.repo, request.stack) ?: return StackPlanCheck.Invalid("There is no stack ${request.stack}")
                request.prs.firstOrNull { it in def.members }?.let { return StackPlanCheck.Invalid("#$it is already in stack ${request.stack}") }
                val top = def.members.mapNotNull { open[it] }.lastOrNull() ?: return StackPlanCheck.Invalid("Stack ${request.stack} has no open members")
                top.headRef to 1
            }
        }
        checkInput(request.prs, minimum, (request as? StackPlanRequest.Arrange)?.trunk)?.let { return StackPlanCheck.Invalid(it) }
        val members: List<PrSummary> = request.prs.map { open.getValue(it) }
        val rewrites = members.mapIndexedNotNull { index, pr ->
            val base = if (index == 0) firstBase else members[index - 1].headRef
            if (pr.baseRef == base) null else StackRewrite(pr.number, pr.headRef)
        }
        return StackPlanCheck.Valid(rewrites)
    }

    override suspend fun planStackRewrite(request: StackPlanRequest): Outcome<StackRewritePlan> =
        host.call(FakeCall.PlanStackRewrite) {
            desktop {
                when (val check = plan(request)) {
                    is StackPlanCheck.Invalid -> invalid(check.reason)
                    is StackPlanCheck.Valid -> Outcome.Ok(StackRewritePlan(check.rewrites, check.rewrites.isNotEmpty()))
                }
            }
        }

    override suspend fun checkStackPlan(request: StackPlanRequest): Outcome<StackPlanCheck> =
        host.call(FakeCall.CheckStackPlan) { Outcome.Ok(plan(request)) }

    override suspend fun stackCandidates(repo: String, stack: Int): Outcome<List<StackCandidate>> =
        host.call(FakeCall.StackCandidates) {
            val def = githubStack(repo, stack) ?: return@call invalid("There is no stack $stack")
            val open = host.openPulls(repo)
            val top = def.members.mapNotNull { n -> open.firstOrNull { it.number == n } }.lastOrNull()
            Outcome.Ok(
                open.filter { it.number !in def.members }.map { pr ->
                    val elsewhere = FakeStacks.samples.firstOrNull { it.repo == repo && pr.number in it.members }
                    val eligibility = when {
                        elsewhere != null -> StackEligibility.Ineligible("Already in ${elsewhere.label}")
                        else -> StackEligibility.Eligible(chained = top != null && pr.baseRef == top.headRef)
                    }
                    StackCandidate(pr.number, pr.title, eligibility)
                },
            )
        }

    private fun start(repo: String, kind: StackJobKind, result: StackJobResult): Outcome<StackJob> {
        val job = StackJob(nextId++, repo, kind, clock.instant(), null, false, StackJobState.Running("Starting"))
        jobs[job.id] = Running(job, result)
        return Outcome.Ok(job)
    }

    /** Arrange and extend run only on exactly the branches the plan would rewrite. */
    private fun confirmed(request: StackPlanRequest, confirm: List<String>): Outcome<List<StackRewrite>> =
        when (val check = plan(request)) {
            is StackPlanCheck.Invalid -> invalid(check.reason)
            is StackPlanCheck.Valid -> if (check.rewrites.map { it.branch }.toSet() == confirm.toSet()) {
                Outcome.Ok(check.rewrites)
            } else {
                Outcome.Err(BackendError.RewriteNotConfirmed(check.rewrites, "the confirmed branches don't match what would be rewritten"))
            }
        }

    override suspend fun makeStack(repo: String, prs: List<Int>, trunk: String): Outcome<StackJob> =
        host.call(FakeCall.MakeStack) {
            desktop {
                when (val check = plan(StackPlanRequest.Arrange(repo, prs, trunk))) {
                    is StackPlanCheck.Invalid -> invalid(check.reason)
                    is StackPlanCheck.Valid -> if (check.rewrites.isNotEmpty()) {
                        invalid("#${check.rewrites.first().number} isn't based on the one below; arrange instead")
                    } else {
                        start(repo, StackJobKind.Make, StackJobResult.Stacked(emptyList(), tracked = true))
                    }
                }
            }
        }

    override suspend fun arrangeStack(repo: String, prs: List<Int>, trunk: String, confirmRewrite: List<String>): Outcome<StackJob> =
        host.call(FakeCall.ArrangeStack) {
            desktop {
                when (val rewrites = confirmed(StackPlanRequest.Arrange(repo, prs, trunk), confirmRewrite)) {
                    is Outcome.Err -> rewrites
                    is Outcome.Ok -> start(repo, StackJobKind.Arrange, StackJobResult.Stacked(rewrites.value.map { it.number }, tracked = true))
                }
            }
        }

    override suspend fun extendStack(repo: String, stack: Int, prs: List<Int>, confirmRewrite: List<String>): Outcome<StackJob> =
        host.call(FakeCall.ExtendStack) {
            desktop {
                when (val rewrites = confirmed(StackPlanRequest.Extend(repo, stack, prs), confirmRewrite)) {
                    is Outcome.Err -> rewrites
                    is Outcome.Ok -> start(repo, StackJobKind.Extend, StackJobResult.Extended(stack, rewrites.value.map { it.number }))
                }
            }
        }

    override suspend fun mergeStack(repo: String, stack: Int, method: StackMergeMethod): Outcome<StackJob> =
        host.call(FakeCall.MergeStack) {
            desktop {
                if (githubStack(repo, stack) == null) invalid("There is no stack $stack") else start(repo, StackJobKind.Merge, StackJobResult.Merged(stack))
            }
        }

    override suspend fun unstack(repo: String, stack: Int): Outcome<StackJob> =
        host.call(FakeCall.Unstack) {
            desktop {
                if (githubStack(repo, stack) == null) invalid("There is no stack $stack") else start(repo, StackJobKind.Unstack, StackJobResult.Unstacked(stack))
            }
        }

    override suspend fun stackJob(id: Long): Outcome<StackJob> = host.call(FakeCall.StackJob) {
        desktop {
            val running = jobs[id] ?: return@desktop Outcome.Err(BackendError.RemoteApi(RemoteErrorCode.NotFound, "no job $id"))
            if (running.job.finished) return@desktop Outcome.Ok(running.job)
            running.polls++
            val job = if (running.polls >= POLLS_TO_FINISH) {
                val state = nextOutcome ?: StackJobState.Done(running.result, "Finished on the desktop")
                nextOutcome = null
                host.emitFeed()
                running.job.copy(finished = true, finishedAt = clock.instant(), state = state)
            } else {
                running.job.copy(state = StackJobState.Running("Working (${running.polls}/$POLLS_TO_FINISH)"))
            }
            jobs[id] = Running(job, running.result, running.polls)
            Outcome.Ok(job)
        }
    }

    companion object {
        const val POLLS_TO_FINISH = 2
    }
}
