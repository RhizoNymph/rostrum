package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/** One branch a stack action would rewrite (rebase and force-push), by pull request. */
data class StackRewrite(val number: Int, val branch: String)

/** What a stack rewrite is planned for: arranging pull requests, or adding them to a stack. */
sealed interface StackPlanRequest {
    val repo: String
    val prs: List<Int>

    /** [prs] bottom first, onto [trunk]. */
    data class Arrange(override val repo: String, override val prs: List<Int>, val trunk: String) : StackPlanRequest

    /** [prs] on top of GitHub stack [stack], in that order. */
    data class Extend(override val repo: String, val stack: Int, override val prs: List<Int>) : StackPlanRequest
}

/** The desktop's dry run: the branches it would rewrite. */
data class StackRewritePlan(val rewrites: List<StackRewrite>, val needsRewrite: Boolean)

/** The local check of a plan, from the cached feed (the desktop's answer is authoritative). */
sealed interface StackPlanCheck {
    data class Valid(val rewrites: List<StackRewrite>) : StackPlanCheck

    data class Invalid(val reason: String) : StackPlanCheck
}

/** Whether a pull request can join a stack. */
sealed interface StackEligibility {
    /** [chained]: its base is already the stack's top head, so nothing is rewritten. */
    data class Eligible(val chained: Boolean) : StackEligibility

    data class Ineligible(val reason: String) : StackEligibility
}

data class StackCandidate(val number: Int, val title: String, val eligibility: StackEligibility)

enum class StackMergeMethod { Merge, Squash, Rebase }

enum class StackJobKind { Make, Arrange, Extend, Merge, Unstack }

/** What a finished stack job did. */
sealed interface StackJobResult {
    data class Stacked(val rewritten: List<Int>, val tracked: Boolean) : StackJobResult

    data class Extended(val stack: Int, val rewritten: List<Int>) : StackJobResult

    data class Merged(val stack: Int) : StackJobResult

    data class Unstacked(val stack: Int) : StackJobResult
}

/** Where a stack job is. Everything but [Running] is final. */
sealed interface StackJobState {
    data class Running(val progress: String?) : StackJobState

    data class Done(val result: StackJobResult, val detail: String) : StackJobState

    /** A rebase stopped on conflicts in [number]; nothing was pushed past it. */
    data class Conflicted(val number: Int, val detail: String) : StackJobState

    /** The conflict went to the desktop's handler in tmux [session], in [worktree]. */
    data class HandedOff(val number: Int, val session: String, val worktree: String, val detail: String) : StackJobState

    /** It stopped; [pushed] are the pull requests already force-pushed. */
    data class Failed(val pushed: List<Int>, val detail: String) : StackJobState
}

/** A stack action running on the paired desktop; poll it by [id]. */
data class StackJob(
    val id: Long,
    val repo: String,
    val kind: StackJobKind,
    val startedAt: Instant,
    val finishedAt: Instant?,
    val finished: Boolean,
    val state: StackJobState,
)
