package io.github.rhizonymph.rostrum.ui.stacks

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
import io.github.rhizonymph.rostrum.data.model.StackSummary
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState

/** GitHub's stack number, for the actions that take one. */
val StackSummary.number: Int? get() = (kind as? StackKind.GitHub)?.number

/** Where a stack action is: a question, a picker, the order, the rewrite confirmation, or the running job. */
sealed interface StackFlow {
    /** Every stack action runs on the paired desktop; there is none. */
    data class NeedsDesktop(val action: String) : StackFlow

    data class ConfirmMerge(
        val repo: String,
        val stack: Int,
        val title: String,
        val members: List<PrSummary>,
        val method: StackMergeMethod = StackMergeMethod.Merge,
        val run: ActionState = ActionState.Idle,
    ) : StackFlow

    data class ConfirmUnstack(val repo: String, val stack: Int, val title: String, val run: ActionState = ActionState.Idle) : StackFlow

    /** A detected chain, made into a GitHub stack as it is (nothing rewritten). */
    data class ConfirmMake(
        val repo: String,
        val members: List<PrSummary>,
        val trunk: String,
        val run: ActionState = ActionState.Idle,
    ) : StackFlow

    /** Pull requests to add on top, in the order picked. */
    data class PickExtend(
        val repo: String,
        val stack: Int,
        val title: String,
        val candidates: UiState<List<StackCandidate>> = UiState.Loading,
        val chosen: List<Int> = emptyList(),
        val run: ActionState = ActionState.Idle,
    ) : StackFlow

    /** The picked pull requests, ordered bottom first, onto a trunk, checked locally as they change. */
    data class OrderArrange(
        val repo: String,
        val members: List<PrSummary>,
        val trunk: String,
        val check: StackPlanCheck? = null,
        val run: ActionState = ActionState.Idle,
    ) : StackFlow

    /**
     * The exact branches the desktop would rewrite, shown before anything is
     * pushed; [note] when the desktop refused a different set.
     */
    data class ConfirmRewrite(
        val request: StackPlanRequest,
        val rewrites: List<StackRewrite>,
        val note: String? = null,
        val run: ActionState = ActionState.Idle,
    ) : StackFlow

    /** A started job, polled until it finishes. */
    data class Job(val job: StackJob) : StackFlow
}

/** Pull requests picked for Arrange, in the order picked. */
data class ArrangeSelection(val repo: String, val picked: List<Int> = emptyList())

/** A candidate can be picked when eligible. */
val StackCandidate.pickable: Boolean get() = eligibility is StackEligibility.Eligible

/** `chains on #11`, `will be rebased`, or why it can't join. */
fun candidateNote(candidate: StackCandidate): String = when (val e = candidate.eligibility) {
    is StackEligibility.Eligible -> if (e.chained) "already based on the top" else "will be rebased onto the top"
    is StackEligibility.Ineligible -> e.reason
}

/** The job's name, for its sheet. */
fun jobTitle(kind: StackJobKind): String = when (kind) {
    StackJobKind.Make -> "Making the stack"
    StackJobKind.Arrange -> "Arranging the stack"
    StackJobKind.Extend -> "Adding to the stack"
    StackJobKind.Merge -> "Merging the stack"
    StackJobKind.Unstack -> "Unstacking"
}

/** What a finished job did, in a sentence (for the snackbar and the sheet). */
fun jobOutcome(job: StackJob): String = when (val state = job.state) {
    is StackJobState.Running -> state.progress ?: "Working on the desktop…"
    is StackJobState.Done -> when (val result = state.result) {
        is StackJobResult.Stacked -> if (result.rewritten.isEmpty()) "Stacked" else "Stacked; rewrote ${numbers(result.rewritten)}"
        is StackJobResult.Extended -> "Added to stack ${result.stack}" + if (result.rewritten.isEmpty()) "" else "; rewrote ${numbers(result.rewritten)}"
        is StackJobResult.Merged -> "Merged stack ${result.stack}"
        is StackJobResult.Unstacked -> "Unstacked stack ${result.stack}"
    }
    is StackJobState.Conflicted -> "Stopped on conflicts in #${state.number}"
    is StackJobState.HandedOff -> "Conflicts in #${state.number} went to tmux session ${state.session}"
    is StackJobState.Failed -> if (state.pushed.isEmpty()) "Failed; nothing was pushed" else "Failed after pushing ${numbers(state.pushed)}"
}

/** The command that picks up a handed-off conflict. */
fun attachCommand(session: String): String = "tmux attach -t $session"

fun numbers(prs: List<Int>): String = prs.joinToString(", ") { "#$it" }

/** The merge confirmation's all-or-nothing note. */
const val MERGE_NOTE = "GitHub merges every pull request in the stack, or none of them."
