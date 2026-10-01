package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.IssueCloseReason
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueStatus
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.StackKind
import io.github.rhizonymph.rostrum.data.model.StackRollup
import io.github.rhizonymph.rostrum.data.model.StackSummary
import uniffi.rostrum_ffi.CloseIssueAs as FfiCloseIssueAs
import uniffi.rostrum_ffi.IssueCloseReason as FfiIssueCloseReason
import uniffi.rostrum_ffi.IssueDetail as FfiIssueDetail
import uniffi.rostrum_ffi.IssueStatus as FfiIssueStatus
import uniffi.rostrum_ffi.IssueSummary as FfiIssueSummary
import uniffi.rostrum_ffi.PullItem as FfiPullItem
import uniffi.rostrum_ffi.StackKind as FfiStackKind
import uniffi.rostrum_ffi.StackRollup as FfiStackRollup
import uniffi.rostrum_ffi.StackSummary as FfiStackSummary

/* Issues and stacks: generated records ↔ model. */

internal fun FfiIssueCloseReason.toModel(): IssueCloseReason = when (this) {
    FfiIssueCloseReason.COMPLETED -> IssueCloseReason.Completed
    FfiIssueCloseReason.NOT_PLANNED -> IssueCloseReason.NotPlanned
    FfiIssueCloseReason.DUPLICATE -> IssueCloseReason.Duplicate
}

internal fun CloseIssueAs.toFfi(): FfiCloseIssueAs = when (this) {
    CloseIssueAs.Completed -> FfiCloseIssueAs.COMPLETED
    CloseIssueAs.NotPlanned -> FfiCloseIssueAs.NOT_PLANNED
}

internal fun FfiIssueStatus.toModel(): IssueStatus = when (this) {
    is FfiIssueStatus.Open -> IssueStatus.Open
    is FfiIssueStatus.Closed -> IssueStatus.Closed(reason?.toModel())
}

internal fun FfiIssueSummary.toModel() = IssueSummary(
    repo = repo,
    number = number.toInt(),
    title = title,
    url = url,
    status = status.toModel(),
    statusChip = statusChip.toModel(),
    author = author?.toModel(),
    createdAt = createdAt,
    updatedAt = updatedAt,
    labels = labels.map { it.toModel() },
    assignees = assignees.map { it.toModel() },
    commentCount = commentCount.toInt(),
    milestone = milestone,
    isYours = isYours,
    assignedToYou = assignedToYou,
)

internal fun FfiIssueDetail.toModel() =
    IssueDetail(issue.toModel(), timeline.map { it.toModel() }, hasEarlier, earlierCount?.toInt())

internal fun FfiStackKind.toModel(): StackKind = when (this) {
    is FfiStackKind.GitHub -> StackKind.GitHub(number.toInt())
    is FfiStackKind.Chain -> StackKind.Chain
}

internal fun FfiStackRollup.toModel() =
    StackRollup(mergeable.toInt(), total.toInt(), worst.toModel(), label, role.toModel())

internal fun FfiStackSummary.toModel() = StackSummary(
    kind = kind.toModel(),
    title = title,
    trunk = trunk,
    memberCount = memberCount.toInt(),
    absent = absent.toInt(),
    rollup = rollup?.toModel(),
)

internal fun FfiPullItem.toModel(): PullItem = when (this) {
    is FfiPullItem.Single -> PullItem.Single(pull.toModel())
    is FfiPullItem.Stack -> PullItem.Stack(stack.toModel(), members.map { it.toModel() })
}
