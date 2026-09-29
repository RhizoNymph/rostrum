package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.CheckRunView
import io.github.rhizonymph.rostrum.data.model.DraftAction
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.MergeVerdict
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.data.model.ThreadCommentView
import io.github.rhizonymph.rostrum.data.model.TimelineEntry
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import uniffi.rostrum_ffi.BranchUpdateMethod as FfiBranchUpdateMethod
import uniffi.rostrum_ffi.CheckRunView as FfiCheckRunView
import uniffi.rostrum_ffi.DraftAction as FfiDraftAction
import uniffi.rostrum_ffi.MergeMethod as FfiMergeMethod
import uniffi.rostrum_ffi.MergeVerdict as FfiMergeVerdict
import uniffi.rostrum_ffi.PullDetail as FfiPullDetail
import uniffi.rostrum_ffi.PullHeader as FfiPullHeader
import uniffi.rostrum_ffi.ReviewThreadView as FfiReviewThreadView
import uniffi.rostrum_ffi.ThreadCommentView as FfiThreadCommentView
import uniffi.rostrum_ffi.TimelineEntry as FfiTimelineEntry
import uniffi.rostrum_ffi.TimelineEvent as FfiTimelineEvent
import uniffi.rostrum_ffi.TimelineKind as FfiTimelineKind

/* One pull request: header, conversation, threads and checks. */

internal fun FfiMergeVerdict.toModel() = MergeVerdict(status.toModel(), sentence, blocksMerge, role.toModel(), chip?.toModel())

internal fun FfiDraftAction.toModel() = DraftAction(toDraft, label)

internal fun FfiPullHeader.toModel() = PullHeader(
    repo = repo,
    number = number.toInt(),
    title = title,
    url = url,
    state = state.toModel(),
    isDraft = isDraft,
    author = author?.toModel(),
    createdAt = createdAt,
    updatedAt = updatedAt,
    headRef = headRef,
    baseRef = baseRef,
    headSha = headSha,
    labels = labels.map { it.toModel() },
    assignees = assignees.map { it.toModel() },
    reviewRequests = reviewRequests.map { it.toModel() },
    reviewDecision = reviewDecision?.toModel(),
    reviewChip = reviewChip?.toModel(),
    merge = merge.toModel(),
    divergence = divergence?.toModel(),
    checks = checks?.toModel(),
    checksRole = checksRole.toModel(),
    changedFiles = changedFiles.toInt(),
    additions = additions.toInt(),
    deletions = deletions.toInt(),
    commentCount = commentCount.toInt(),
    isYours = isYours,
    reviewRequested = reviewRequested,
    draftAction = draftAction.toModel(),
)

internal fun FfiTimelineEvent.toModel(): TimelineEvent = when (this) {
    is FfiTimelineEvent.Merged -> TimelineEvent.Merged
    is FfiTimelineEvent.Closed -> TimelineEvent.Closed
    is FfiTimelineEvent.Reopened -> TimelineEvent.Reopened
    is FfiTimelineEvent.ReadyForReview -> TimelineEvent.ReadyForReview
    is FfiTimelineEvent.ConvertedToDraft -> TimelineEvent.ConvertedToDraft
    is FfiTimelineEvent.ForcePushed -> TimelineEvent.ForcePushed
    is FfiTimelineEvent.ReviewRequested -> TimelineEvent.ReviewRequested(reviewer)
    is FfiTimelineEvent.Assigned -> TimelineEvent.Assigned(assignee)
    is FfiTimelineEvent.Labeled -> TimelineEvent.Labeled(label)
    is FfiTimelineEvent.Unlabeled -> TimelineEvent.Unlabeled(label)
    is FfiTimelineEvent.Renamed -> TimelineEvent.Renamed(from, to)
    is FfiTimelineEvent.Other -> TimelineEvent.Other(kind)
}

internal fun FfiTimelineKind.toModel(): TimelineKind = when (this) {
    is FfiTimelineKind.Description -> TimelineKind.Description(body.toModel(), source)
    is FfiTimelineKind.Comment -> TimelineKind.Comment(body.toModel(), source)
    is FfiTimelineKind.Review -> TimelineKind.Review(state.toModel(), chip.toModel(), body.toModel(), source, threadIds)
    is FfiTimelineKind.Event -> TimelineKind.Event(event.toModel(), text)
}

internal fun FfiTimelineEntry.toModel() = TimelineEntry(id, author?.toModel(), createdAt, kind.toModel())

internal fun FfiThreadCommentView.toModel() = ThreadCommentView(id, author?.toModel(), createdAt, body.toModel(), source)

internal fun FfiReviewThreadView.toModel() = ReviewThreadView(
    id = id,
    path = path,
    line = line?.toInt(),
    originalLine = originalLine?.toInt(),
    side = side.toModel(),
    resolved = resolved,
    outdated = outdated,
    location = location,
    comments = comments.map { it.toModel() },
    canReply = canReply,
)

internal fun FfiCheckRunView.toModel() = CheckRunView(name, state?.toModel(), role.toModel(), statusText, url)

internal fun FfiPullDetail.toModel() = PullDetail(
    header = header.toModel(),
    timeline = timeline.map { it.toModel() },
    threads = threads.map { it.toModel() },
    checks = checks.map { it.toModel() },
    unresolvedThreads = unresolvedThreads.toInt(),
    pendingReview = pendingReview.toModel(),
)

internal fun MergeMethod.toFfi(): FfiMergeMethod = when (this) {
    MergeMethod.Merge -> FfiMergeMethod.MERGE
    MergeMethod.Squash -> FfiMergeMethod.SQUASH
    MergeMethod.Rebase -> FfiMergeMethod.REBASE
}

internal fun BranchUpdateMethod.toFfi(): FfiBranchUpdateMethod = when (this) {
    BranchUpdateMethod.Merge -> FfiBranchUpdateMethod.MERGE
    BranchUpdateMethod.Rebase -> FfiBranchUpdateMethod.REBASE
}
