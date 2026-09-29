package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.ChangedFile
import io.github.rhizonymph.rostrum.data.model.CodeSegment
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DiffAvailability
import io.github.rhizonymph.rostrum.data.model.DiffLineView
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.DiffStats
import io.github.rhizonymph.rostrum.data.model.DraftAnchor
import io.github.rhizonymph.rostrum.data.model.FileDiff
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.FileStatus
import io.github.rhizonymph.rostrum.data.model.FilesOverview
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.data.model.MapColumn
import io.github.rhizonymph.rostrum.data.model.MapTile
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.RankedFile
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.data.model.TileHeat
import uniffi.rostrum_ffi.ChangedFile as FfiChangedFile
import uniffi.rostrum_ffi.CodeSegment as FfiCodeSegment
import uniffi.rostrum_ffi.CommentAnchor as FfiCommentAnchor
import uniffi.rostrum_ffi.DiffAvailability as FfiDiffAvailability
import uniffi.rostrum_ffi.DiffLineView as FfiDiffLineView
import uniffi.rostrum_ffi.DiffRow as FfiDiffRow
import uniffi.rostrum_ffi.DiffStats as FfiDiffStats
import uniffi.rostrum_ffi.DraftAnchor as FfiDraftAnchor
import uniffi.rostrum_ffi.FileDiff as FfiFileDiff
import uniffi.rostrum_ffi.FileDiffBody as FfiFileDiffBody
import uniffi.rostrum_ffi.FileStatus as FfiFileStatus
import uniffi.rostrum_ffi.FilesOverview as FfiFilesOverview
import uniffi.rostrum_ffi.LineKind as FfiLineKind
import uniffi.rostrum_ffi.MapColumn as FfiMapColumn
import uniffi.rostrum_ffi.MapTile as FfiMapTile
import uniffi.rostrum_ffi.PendingReview as FfiPendingReview
import uniffi.rostrum_ffi.RankedFile as FfiRankedFile
import uniffi.rostrum_ffi.ReviewDraft as FfiReviewDraft
import uniffi.rostrum_ffi.ReviewEvent as FfiReviewEvent
import uniffi.rostrum_ffi.TileHeat as FfiTileHeat

/* The Files tab, one file's diff, and the pending review. */

internal fun FfiDiffStats.toModel() = DiffStats(
    files = files.toInt(),
    additions = additions.toLong(),
    deletions = deletions.toLong(),
    addedFiles = addedFiles.toInt(),
    removedFiles = removedFiles.toInt(),
    renamedFiles = renamedFiles.toInt(),
    modifiedFiles = modifiedFiles.toInt(),
)

internal fun FfiTileHeat.toModel() = TileHeat(removedRatio, alpha)

internal fun FfiMapTile.toModel() = MapTile(
    fileIndex = fileIndex?.toInt(),
    label = label,
    additions = additions.toLong(),
    deletions = deletions.toLong(),
    share = share,
    heat = heat.toModel(),
)

internal fun FfiMapColumn.toModel() = MapColumn(
    label = label,
    additions = additions.toLong(),
    deletions = deletions.toLong(),
    fileCount = fileCount.toInt(),
    share = share,
    tiles = tiles.map { it.toModel() },
)

internal fun FfiFileStatus.toModel(): FileStatus = when (this) {
    FfiFileStatus.ADDED -> FileStatus.Added
    FfiFileStatus.REMOVED -> FileStatus.Removed
    FfiFileStatus.MODIFIED -> FileStatus.Modified
    FfiFileStatus.RENAMED -> FileStatus.Renamed
    FfiFileStatus.COPIED -> FileStatus.Copied
    FfiFileStatus.CHANGED -> FileStatus.Changed
    FfiFileStatus.UNCHANGED -> FileStatus.Unchanged
}

internal fun FfiDiffAvailability.toModel(): DiffAvailability = when (this) {
    FfiDiffAvailability.TEXT -> DiffAvailability.Text
    FfiDiffAvailability.BINARY -> DiffAvailability.Binary
    FfiDiffAvailability.TOO_LARGE -> DiffAvailability.TooLarge
    FfiDiffAvailability.UNPARSEABLE -> DiffAvailability.Unparseable
    FfiDiffAvailability.NO_TEXT_CHANGES -> DiffAvailability.NoTextChanges
}

internal fun FfiRankedFile.toModel() = RankedFile(
    fileIndex = fileIndex.toInt(),
    path = path,
    status = status.toModel(),
    additions = additions.toInt(),
    deletions = deletions.toInt(),
    additionsShare = additionsShare,
    deletionsShare = deletionsShare,
)

internal fun FfiChangedFile.toModel() = ChangedFile(
    index = index.toInt(),
    path = path,
    previousPath = previousPath,
    status = status.toModel(),
    additions = additions.toInt(),
    deletions = deletions.toInt(),
    availability = availability.toModel(),
    threads = threads.toInt(),
    drafts = drafts.toInt(),
)

internal fun FfiFilesOverview.toModel() = FilesOverview(
    headSha = headSha,
    stats = stats.toModel(),
    changeMap = changeMap.map { it.toModel() },
    ranked = ranked.map { it.toModel() },
    files = files.map { it.toModel() },
)

internal fun FfiLineKind.toModel(): LineKind = when (this) {
    FfiLineKind.CONTEXT -> LineKind.Context
    FfiLineKind.ADDED -> LineKind.Added
    FfiLineKind.REMOVED -> LineKind.Removed
}

internal fun FfiCodeSegment.toModel() = CodeSegment(text, color.toInt(), bold, italic, emphasized)

internal fun FfiCommentAnchor.toModel() = CommentAnchor(path, line.toInt(), side.toModel())

/** Anchors only ever come from the core; this hands one back unchanged. */
internal fun CommentAnchor.toFfi() = FfiCommentAnchor(path, line.toUInt(), side.toFfi())

internal fun FfiDiffLineView.toModel() = DiffLineView(
    kind = kind.toModel(),
    oldLine = oldLine?.toInt(),
    newLine = newLine?.toInt(),
    segments = segments.map { it.toModel() },
    anchor = anchor?.toModel(),
    noNewlineAtEof = noNewlineAtEof,
)

internal fun FfiDiffRow.toModel(): DiffRow = when (this) {
    is FfiDiffRow.Hunk -> DiffRow.Hunk(index.toInt(), header)
    is FfiDiffRow.Line -> DiffRow.Line(line.toModel())
    is FfiDiffRow.Thread -> DiffRow.Thread(thread.toModel())
    is FfiDiffRow.Draft -> DiffRow.Draft(draft.toModel())
}

internal fun FfiFileDiffBody.toModel(): FileDiffBody = when (this) {
    is FfiFileDiffBody.Rows -> FileDiffBody.Rows(rows.map { it.toModel() })
    is FfiFileDiffBody.Unavailable -> FileDiffBody.Unavailable
}

internal fun FfiFileDiff.toModel() = FileDiff(file.toModel(), headSha, body.toModel())

internal fun FfiDraftAnchor.toModel() = DraftAnchor(path, line.toInt(), side.toModel(), startLine?.toInt())

internal fun FfiReviewDraft.toModel() = ReviewDraft(id.toLong(), anchor.toModel(), body, location)

internal fun FfiPendingReview.toModel() = PendingReview(
    repo = repo,
    number = number.toInt(),
    drafts = drafts.map { it.toModel() },
    draftedAgainst = draftedAgainst,
    headSha = headSha,
    stale = stale,
)

internal fun ReviewEvent.toFfi(): FfiReviewEvent = when (this) {
    ReviewEvent.Comment -> FfiReviewEvent.COMMENT
    ReviewEvent.Approve -> FfiReviewEvent.APPROVE
    ReviewEvent.RequestChanges -> FfiReviewEvent.REQUEST_CHANGES
}
