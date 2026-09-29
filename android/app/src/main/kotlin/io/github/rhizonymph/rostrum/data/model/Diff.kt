package io.github.rhizonymph.rostrum.data.model

/** The Files tab's landing view (`diff/types.rs`). */
data class FilesOverview(
    /** The commit these files describe. */
    val headSha: String,
    val stats: DiffStats,
    /** Directories as columns, sized by their share of the churn. */
    val changeMap: List<MapColumn>,
    /** Every file, largest change first. */
    val ranked: List<RankedFile>,
    /** Every file in patch order; `index` is what fileDiff takes. */
    val files: List<ChangedFile>,
)

/** Totals for the summary strip. */
data class DiffStats(
    val files: Int,
    val additions: Long,
    val deletions: Long,
    val addedFiles: Int,
    val removedFiles: Int,
    val renamedFiles: Int,
    val modifiedFiles: Int,
)

/** One column of the change map: a directory, `(root)`, or `(other)`. */
data class MapColumn(
    val label: String,
    val additions: Long,
    val deletions: Long,
    val fileCount: Int,
    /** Fraction of the map's width; columns sum to 1. */
    val share: Float,
    val tiles: List<MapTile>,
)

/** One tile of a column: a file, or the `+N more` aggregate. */
data class MapTile(
    /** The file to open on tap; `null` for the aggregate tile. */
    val fileIndex: Int?,
    /** The file name, or `+N more`. */
    val label: String,
    val additions: Long,
    val deletions: Long,
    /** Fraction of the column's height; tiles sum to 1. */
    val share: Float,
    val heat: TileHeat,
)

/** Blend success (added) and danger (removed) by [removedRatio], at [alpha]. */
data class TileHeat(
    /** 0 is all additions, 1 all deletions; `null` for no line changes. */
    val removedRatio: Float?,
    val alpha: Float,
)

/** One row of the "largest changes" list. */
data class RankedFile(
    val fileIndex: Int,
    val path: String,
    val status: FileStatus,
    val additions: Int,
    val deletions: Int,
    /** Widths of the bar's green and red parts, as fractions of the largest churn. */
    val additionsShare: Float,
    val deletionsShare: Float,
)

/** One changed file. */
data class ChangedFile(
    val index: Int,
    val path: String,
    /** The old path, for renames and copies. */
    val previousPath: String?,
    val status: FileStatus,
    val additions: Int,
    val deletions: Int,
    val availability: DiffAvailability,
    /** Inline threads anchored to lines of this file's diff. */
    val threads: Int,
    /** Your pending drafts on this file. */
    val drafts: Int,
)

enum class FileStatus { Added, Removed, Modified, Renamed, Copied, Changed, Unchanged }

/** Whether a file's diff can be shown, and if not, why. */
enum class DiffAvailability { Text, Binary, TooLarge, Unparseable, NoTextChanges }

/** One file's diff. */
data class FileDiff(
    val file: ChangedFile,
    val headSha: String,
    val body: FileDiffBody,
)

sealed interface FileDiffBody {
    data class Rows(val rows: List<DiffRow>) : FileDiffBody

    /** Nothing to show; `file.availability` says why. */
    data object Unavailable : FileDiffBody
}

/** One row of a file's diff, in display order. */
sealed interface DiffRow {
    /** A hunk's `@@ -a,b +c,d @@ context` line. */
    data class Hunk(val index: Int, val header: String) : DiffRow

    data class Line(val line: DiffLineView) : DiffRow

    /** An existing thread, placed right after the line it is anchored to. */
    data class Thread(val thread: ReviewThreadView) : DiffRow

    /** One of your pending drafts, placed right after its (last) line. */
    data class Draft(val draft: ReviewDraft) : DiffRow
}

enum class LineKind { Context, Added, Removed }

/** One line of code. */
data class DiffLineView(
    val kind: LineKind,
    /** Line number in the old file; `null` for added lines. */
    val oldLine: Int?,
    /** Line number in the new file; `null` for removed lines. */
    val newLine: Int?,
    /** The line's text in styled runs, without the `+`/`-` marker. */
    val segments: List<CodeSegment>,
    /** Where a comment on this line attaches, or `null` if it cannot take one. */
    val anchor: CommentAnchor?,
    val noNewlineAtEof: Boolean,
) {
    val text: String get() = segments.joinToString("") { it.text }
}

/** A run of code with one style. [argb] is the syntax colour, opaque ARGB. */
data class CodeSegment(
    val text: String,
    val argb: Int,
    val bold: Boolean = false,
    val italic: Boolean = false,
    /** Part of the words that changed: draw a stronger background. */
    val emphasized: Boolean = false,
)

/** Where a review comment attaches: GitHub's `path` + `line` + `side`. */
data class CommentAnchor(val path: String, val line: Int, val side: Side)
