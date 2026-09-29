package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.ChangedFile
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DiffAvailability
import io.github.rhizonymph.rostrum.data.model.DiffLineView
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.DiffStats
import io.github.rhizonymph.rostrum.data.model.FileStatus
import io.github.rhizonymph.rostrum.data.model.FilesOverview
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.data.model.MapColumn
import io.github.rhizonymph.rostrum.data.model.MapTile
import io.github.rhizonymph.rostrum.data.model.RankedFile
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.data.model.TileHeat

/** A changed file as the fake stores it: stats plus hunks of raw lines. */
internal data class FakeFile(
    val path: String,
    val status: FileStatus,
    val additions: Int,
    val deletions: Int,
    val hunks: List<FakeHunk>,
    val availability: DiffAvailability = DiffAvailability.Text,
    val previousPath: String? = null,
) {
    val churn: Int get() = additions + deletions
}

internal data class FakeHunk(
    val oldStart: Int,
    val newStart: Int,
    val context: String,
    val lines: List<Pair<LineKind, String>>,
) {
    val header: String
        get() {
            val oldCount = lines.count { it.first != LineKind.Added }
            val newCount = lines.count { it.first != LineKind.Removed }
            val suffix = if (context.isBlank()) "" else " $context"
            return "@@ -$oldStart,$oldCount +$newStart,$newCount @@$suffix"
        }
}

internal class HunkBuilder {
    val lines = mutableListOf<Pair<LineKind, String>>()
    fun ctx(text: String) { lines += LineKind.Context to text }
    fun del(text: String) { lines += LineKind.Removed to text }
    fun add(text: String) { lines += LineKind.Added to text }
}

internal fun hunk(oldStart: Int, newStart: Int, context: String = "", build: HunkBuilder.() -> Unit): FakeHunk =
    FakeHunk(oldStart, newStart, context, HunkBuilder().apply(build).lines)

/** A file that exists only on the new side, lines 1.. as given. */
internal fun addedFile(path: String, totalAdditions: Int, lines: List<String>): FakeFile = FakeFile(
    path = path,
    status = FileStatus.Added,
    additions = totalAdditions,
    deletions = 0,
    hunks = listOf(FakeHunk(0, 1, "", lines.map { LineKind.Added to it })),
)

/** The diff machinery the fake shares between every pull request. */
internal object FakeDiffs {
    /** One file's rows: hunks, lines with anchors, and threads/drafts after their lines. */
    fun rows(file: FakeFile, threads: List<ReviewThreadView>, drafts: List<ReviewDraft>): List<DiffRow> {
        val out = mutableListOf<DiffRow>()
        val fileThreads = threads.filter { it.path == file.path && it.line != null }
        val fileDrafts = drafts.filter { it.anchor.path == file.path }
        file.hunks.forEachIndexed { index, hunk ->
            out += DiffRow.Hunk(index, hunk.header)
            var old = hunk.oldStart
            var new = hunk.newStart
            for ((kind, text) in hunk.lines) {
                val (oldLine, newLine) = when (kind) {
                    LineKind.Context -> (old++) to (new++)
                    LineKind.Added -> null to (new++)
                    LineKind.Removed -> (old++) to null
                }
                val anchor = anchorFor(file.path, kind, oldLine, newLine)
                out += DiffRow.Line(
                    DiffLineView(
                        kind = kind,
                        oldLine = oldLine,
                        newLine = newLine,
                        segments = FakeHighlighter.highlight(file.path, text),
                        anchor = anchor,
                        noNewlineAtEof = false,
                    ),
                )
                fileThreads.filter { it.side == anchor.side && it.line == anchor.line }
                    .forEach { out += DiffRow.Thread(it) }
                fileDrafts.filter { it.anchor.side == anchor.side && it.anchor.line == anchor.line }
                    .forEach { out += DiffRow.Draft(it) }
            }
        }
        return out
    }

    /** Every anchor a comment on [file] may use. */
    fun anchors(file: FakeFile): Set<CommentAnchor> =
        rows(file, emptyList(), emptyList()).mapNotNullTo(mutableSetOf()) { (it as? DiffRow.Line)?.line?.anchor }

    private fun anchorFor(path: String, kind: LineKind, oldLine: Int?, newLine: Int?): CommentAnchor = when (kind) {
        LineKind.Removed -> CommentAnchor(path, requireNotNull(oldLine), Side.Left)
        LineKind.Added, LineKind.Context -> CommentAnchor(path, requireNotNull(newLine), Side.Right)
    }

    fun changedFile(index: Int, file: FakeFile, threads: Int, drafts: Int) = ChangedFile(
        index = index,
        path = file.path,
        previousPath = file.previousPath,
        status = file.status,
        additions = file.additions,
        deletions = file.deletions,
        availability = file.availability,
        threads = threads,
        drafts = drafts,
    )

    /** The overview: stats, a change map by directory, and files ranked by churn. */
    fun overview(
        headSha: String,
        files: List<FakeFile>,
        threads: List<ReviewThreadView>,
        drafts: List<ReviewDraft>,
    ): FilesOverview {
        val changed = files.mapIndexed { index, file ->
            changedFile(
                index = index,
                file = file,
                threads = threads.count { it.path == file.path },
                drafts = drafts.count { it.anchor.path == file.path },
            )
        }
        val stats = DiffStats(
            files = files.size,
            additions = files.sumOf { it.additions.toLong() },
            deletions = files.sumOf { it.deletions.toLong() },
            addedFiles = files.count { it.status == FileStatus.Added },
            removedFiles = files.count { it.status == FileStatus.Removed },
            renamedFiles = files.count { it.status == FileStatus.Renamed },
            modifiedFiles = files.count { it.status !in setOf(FileStatus.Added, FileStatus.Removed, FileStatus.Renamed) },
        )
        val weight = { f: FakeFile -> maxOf(f.churn, 1).toFloat() }
        val total = files.sumOf { weight(it).toDouble() }.toFloat().coerceAtLeast(1f)
        val maxChurn = files.maxOfOrNull { weight(it) } ?: 1f
        val columns = files.withIndex()
            .groupBy { directoryOf(it.value.path) }
            .map { (dir, members) ->
                val sorted = members.sortedByDescending { weight(it.value) }
                val columnWeight = sorted.sumOf { weight(it.value).toDouble() }.toFloat()
                MapColumn(
                    label = dir.removePrefix("crates/"),
                    additions = sorted.sumOf { it.value.additions.toLong() },
                    deletions = sorted.sumOf { it.value.deletions.toLong() },
                    fileCount = sorted.size,
                    share = columnWeight / total,
                    tiles = sorted.map { (index, file) ->
                        MapTile(
                            fileIndex = index,
                            label = file.path.substringAfterLast('/'),
                            additions = file.additions.toLong(),
                            deletions = file.deletions.toLong(),
                            share = weight(file) / columnWeight,
                            heat = TileHeat(
                                removedRatio = if (file.churn == 0) null else file.deletions.toFloat() / file.churn,
                                alpha = 0.22f + 0.3f * (weight(file) / maxChurn),
                            ),
                        )
                    },
                )
            }
            .sortedByDescending { it.share }
        val ranked = files.withIndex()
            .sortedByDescending { it.value.churn }
            .map { (index, file) ->
                RankedFile(
                    fileIndex = index,
                    path = file.path,
                    status = file.status,
                    additions = file.additions,
                    deletions = file.deletions,
                    additionsShare = file.additions / maxChurn,
                    deletionsShare = file.deletions / maxChurn,
                )
            }
        return FilesOverview(headSha, stats, columns, ranked, changed)
    }

    private fun directoryOf(path: String): String =
        if ('/' in path) path.substringBeforeLast('/') else "(root)"
}
