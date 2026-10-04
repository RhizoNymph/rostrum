package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.StackCandidate
import io.github.rhizonymph.rostrum.data.model.StackEligibility
import io.github.rhizonymph.rostrum.data.model.StackJob
import io.github.rhizonymph.rostrum.data.model.StackJobKind
import io.github.rhizonymph.rostrum.data.model.StackJobResult
import io.github.rhizonymph.rostrum.data.model.StackJobState
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanCheck
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.data.model.StackRewrite
import io.github.rhizonymph.rostrum.data.model.StackRewritePlan
import uniffi.rostrum_ffi.StackCandidate as FfiStackCandidate
import uniffi.rostrum_ffi.StackEligibility as FfiStackEligibility
import uniffi.rostrum_ffi.StackJob as FfiStackJob
import uniffi.rostrum_ffi.StackJobKind as FfiStackJobKind
import uniffi.rostrum_ffi.StackJobResult as FfiStackJobResult
import uniffi.rostrum_ffi.StackJobState as FfiStackJobState
import uniffi.rostrum_ffi.StackMergeMethod as FfiStackMergeMethod
import uniffi.rostrum_ffi.StackPlanCheck as FfiStackPlanCheck
import uniffi.rostrum_ffi.StackPlanRequest as FfiStackPlanRequest
import uniffi.rostrum_ffi.StackRewrite as FfiStackRewrite
import uniffi.rostrum_ffi.StackRewritePlan as FfiStackRewritePlan

/* Stack actions through the desktop: generated records ↔ model. */

internal fun FfiStackRewrite.toModel() = StackRewrite(number.toInt(), branch)

internal fun FfiStackRewritePlan.toModel() = StackRewritePlan(rewrites.map { it.toModel() }, needsRewrite)

internal fun StackPlanRequest.toFfi(): FfiStackPlanRequest = when (this) {
    is StackPlanRequest.Arrange -> FfiStackPlanRequest.Arrange(repo, prs.map { it.toUInt() }, trunk)
    is StackPlanRequest.Extend -> FfiStackPlanRequest.Extend(repo, stack.toUInt(), prs.map { it.toUInt() })
}

internal fun FfiStackPlanCheck.toModel(): StackPlanCheck = when (this) {
    is FfiStackPlanCheck.Valid -> StackPlanCheck.Valid(rewrites.map { it.toModel() })
    is FfiStackPlanCheck.Invalid -> StackPlanCheck.Invalid(reason)
}

internal fun FfiStackEligibility.toModel(): StackEligibility = when (this) {
    is FfiStackEligibility.Eligible -> StackEligibility.Eligible(chained)
    is FfiStackEligibility.Ineligible -> StackEligibility.Ineligible(reason)
}

internal fun FfiStackCandidate.toModel() = StackCandidate(number.toInt(), title, eligibility.toModel())

internal fun StackMergeMethod.toFfi(): FfiStackMergeMethod = when (this) {
    StackMergeMethod.Merge -> FfiStackMergeMethod.MERGE
    StackMergeMethod.Squash -> FfiStackMergeMethod.SQUASH
    StackMergeMethod.Rebase -> FfiStackMergeMethod.REBASE
}

internal fun FfiStackJobKind.toModel(): StackJobKind = when (this) {
    FfiStackJobKind.MAKE -> StackJobKind.Make
    FfiStackJobKind.ARRANGE -> StackJobKind.Arrange
    FfiStackJobKind.EXTEND -> StackJobKind.Extend
    FfiStackJobKind.MERGE -> StackJobKind.Merge
    FfiStackJobKind.UNSTACK -> StackJobKind.Unstack
}

internal fun FfiStackJobResult.toModel(): StackJobResult = when (this) {
    is FfiStackJobResult.Stacked -> StackJobResult.Stacked(rewritten.map { it.toInt() }, tracked)
    is FfiStackJobResult.Extended -> StackJobResult.Extended(stack.toInt(), rewritten.map { it.toInt() })
    is FfiStackJobResult.Merged -> StackJobResult.Merged(stack.toInt())
    is FfiStackJobResult.Unstacked -> StackJobResult.Unstacked(stack.toInt())
}

internal fun FfiStackJobState.toModel(): StackJobState = when (this) {
    is FfiStackJobState.Running -> StackJobState.Running(progress)
    is FfiStackJobState.Done -> StackJobState.Done(result.toModel(), detail)
    is FfiStackJobState.Conflicted -> StackJobState.Conflicted(number.toInt(), detail)
    is FfiStackJobState.HandedOff -> StackJobState.HandedOff(number.toInt(), session, worktree, detail)
    is FfiStackJobState.Failed -> StackJobState.Failed(pushed.map { it.toInt() }, detail)
}

internal fun FfiStackJob.toModel() = StackJob(
    id = id.toLong(),
    repo = repo,
    kind = kind.toModel(),
    startedAt = startedAt,
    finishedAt = finishedAt,
    finished = finished,
    state = state.toModel(),
)
