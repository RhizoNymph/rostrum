package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.MdSpan
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.data.model.ReviewState
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.data.model.UserRef
import uniffi.rostrum_ffi.CheckState as FfiCheckState
import uniffi.rostrum_ffi.Chip as FfiChip
import uniffi.rostrum_ffi.ColorRole as FfiColorRole
import uniffi.rostrum_ffi.LabelView as FfiLabelView
import uniffi.rostrum_ffi.MdBlock as FfiMdBlock
import uniffi.rostrum_ffi.MdBlockKind as FfiMdBlockKind
import uniffi.rostrum_ffi.MdSpan as FfiMdSpan
import uniffi.rostrum_ffi.MergeStatus as FfiMergeStatus
import uniffi.rostrum_ffi.PullState as FfiPullState
import uniffi.rostrum_ffi.ReviewDecision as FfiReviewDecision
import uniffi.rostrum_ffi.ReviewState as FfiReviewState
import uniffi.rostrum_ffi.Side as FfiSide
import uniffi.rostrum_ffi.UserRef as FfiUserRef

/*
 * Generated records → the app's model, for the types every area shares.
 * Numbers narrow from UniFFI's unsigned types (`u32` → Int, `u64` → Long);
 * ARGB colours keep their bit pattern.
 */

internal fun FfiUserRef.toModel() = UserRef(login, avatarUrl)

internal fun FfiColorRole.toModel(): ColorRole = when (this) {
    FfiColorRole.SUCCESS -> ColorRole.Success
    FfiColorRole.WARNING -> ColorRole.Warning
    FfiColorRole.DANGER -> ColorRole.Danger
    FfiColorRole.DRAFT -> ColorRole.Draft
    FfiColorRole.ACCENT -> ColorRole.Accent
    FfiColorRole.NEUTRAL -> ColorRole.Neutral
}

internal fun FfiChip.toModel() = Chip(text, role.toModel(), tooltip)

internal fun FfiLabelView.toModel() = LabelView(name, color?.toInt())

internal fun FfiSide.toModel(): Side = when (this) {
    FfiSide.LEFT -> Side.Left
    FfiSide.RIGHT -> Side.Right
}

internal fun Side.toFfi(): FfiSide = when (this) {
    Side.Left -> FfiSide.LEFT
    Side.Right -> FfiSide.RIGHT
}

internal fun FfiCheckState.toModel(): CheckState = when (this) {
    FfiCheckState.EXPECTED -> CheckState.Expected
    FfiCheckState.ERROR -> CheckState.Error
    FfiCheckState.FAILURE -> CheckState.Failure
    FfiCheckState.PENDING -> CheckState.Pending
    FfiCheckState.SUCCESS -> CheckState.Success
}

internal fun FfiReviewDecision.toModel(): ReviewDecision = when (this) {
    FfiReviewDecision.APPROVED -> ReviewDecision.Approved
    FfiReviewDecision.CHANGES_REQUESTED -> ReviewDecision.ChangesRequested
    FfiReviewDecision.REVIEW_REQUIRED -> ReviewDecision.ReviewRequired
}

internal fun FfiMergeStatus.toModel(): MergeStatus = when (this) {
    FfiMergeStatus.COMPUTING -> MergeStatus.Computing
    FfiMergeStatus.CONFLICTS -> MergeStatus.Conflicts
    FfiMergeStatus.DRAFT -> MergeStatus.Draft
    FfiMergeStatus.BLOCKED -> MergeStatus.Blocked
    FfiMergeStatus.BEHIND -> MergeStatus.Behind
    FfiMergeStatus.UNSTABLE -> MergeStatus.Unstable
    FfiMergeStatus.READY -> MergeStatus.Ready
}

internal fun FfiReviewState.toModel(): ReviewState = when (this) {
    FfiReviewState.PENDING -> ReviewState.Pending
    FfiReviewState.COMMENTED -> ReviewState.Commented
    FfiReviewState.APPROVED -> ReviewState.Approved
    FfiReviewState.CHANGES_REQUESTED -> ReviewState.ChangesRequested
    FfiReviewState.DISMISSED -> ReviewState.Dismissed
}

internal fun FfiPullState.toModel(): PullState = when (this) {
    FfiPullState.OPEN -> PullState.Open
    FfiPullState.CLOSED -> PullState.Closed
    FfiPullState.MERGED -> PullState.Merged
}

// --- markdown -----------------------------------------------------------------

internal fun FfiMdSpan.toModel() = MdSpan(text, bold, italic, code, strike, link)

internal fun FfiMdBlockKind.toModel(): MdBlockKind = when (this) {
    is FfiMdBlockKind.Paragraph -> MdBlockKind.Paragraph
    is FfiMdBlockKind.Heading -> MdBlockKind.Heading(level.toInt())
    is FfiMdBlockKind.Code -> MdBlockKind.Code(language, code)
    is FfiMdBlockKind.ListItem -> MdBlockKind.ListItem(ordered, number.toLong(), checked)
    is FfiMdBlockKind.Rule -> MdBlockKind.Rule
    is FfiMdBlockKind.TableRow -> MdBlockKind.TableRow(cells.map { cell -> cell.map { it.toModel() } }, header)
    is FfiMdBlockKind.Image -> MdBlockKind.Image(url, alt)
}

internal fun FfiMdBlock.toModel() = MdBlock(
    kind = kind.toModel(),
    spans = spans.map { it.toModel() },
    quoteDepth = quoteDepth.toInt(),
    listDepth = listDepth.toInt(),
)

internal fun List<FfiMdBlock>.toModel(): List<MdBlock> = map { it.toModel() }
