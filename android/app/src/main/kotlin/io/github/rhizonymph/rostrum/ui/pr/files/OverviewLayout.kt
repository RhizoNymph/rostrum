package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.DiffStats
import io.github.rhizonymph.rostrum.data.model.FileStatus
import io.github.rhizonymph.rostrum.data.model.MapTile
import io.github.rhizonymph.rostrum.data.model.RankedFile
import io.github.rhizonymph.rostrum.data.model.TileHeat

/**
 * Pure layout and wording for the Files overview: how the change map's
 * columns and tiles split the space, what a tile has room to show, how it is
 * coloured, and the summary strip. Sizes are in dp as plain floats.
 */

/**
 * Splits [available] among items in proportion to [shares], leaving [gap]
 * between neighbours and giving every item at least [min] (the slivers of a
 * change map stay visible and tappable). The results sum to the space left
 * after the gaps. When even [min] each does not fit, the space is split evenly.
 */
fun distribute(shares: List<Float>, available: Float, gap: Float, min: Float): List<Float> {
    val n = shares.size
    if (n == 0) return emptyList()
    val space = (available - gap * (n - 1)).coerceAtLeast(0f)
    if (min * n >= space) return List(n) { space / n }
    val fixed = BooleanArray(n)
    while (true) {
        val fixedCount = fixed.count { it }
        val free = space - min * fixedCount
        val freeShare = shares.indices.filter { !fixed[it] }.sumOf { shares[it].coerceAtLeast(0f).toDouble() }.toFloat()
        val sizes = shares.mapIndexed { i, share ->
            when {
                fixed[i] -> min
                freeShare > 0f -> free * share.coerceAtLeast(0f) / freeShare
                else -> free / (n - fixedCount)
            }
        }
        val tooSmall = sizes.indices.filter { !fixed[it] && sizes[it] < min }
        if (tooSmall.isEmpty()) return sizes
        tooSmall.forEach { fixed[it] = true }
    }
}

/** The change map's geometry, as the mockup draws it. */
object ChangeMapMetrics {
    const val HEIGHT = 250f
    const val PADDING = 8f
    const val COLUMN_GAP = 4f
    const val TILE_GAP = 3f
    const val LABEL_HEIGHT = 14f
    const val LABEL_GAP = 4f
    const val MIN_COLUMN = 8f
    const val MIN_TILE = 8f
    /** Columns narrower than this drop their directory label. */
    const val LABEL_MIN_WIDTH = 24f
}

/** What a tile has room to show. */
enum class TileContent {
    /** Name above the counts. */
    Stacked,

    /** Name and counts on one line. */
    Inline,

    /** Name and additions rotated, for a tall narrow tile. */
    Vertical,

    /** Colour only. */
    None,
}

fun tileContent(width: Float, height: Float): TileContent = when {
    width >= 64f && height >= 52f -> TileContent.Stacked
    width >= 64f && height >= 18f -> TileContent.Inline
    width >= 20f && height >= 72f -> TileContent.Vertical
    else -> TileContent.None
}

fun showsColumnLabel(width: Float): Boolean = width >= ChangeMapMetrics.LABEL_MIN_WIDTH

/**
 * A tile's fill: the added colour blended toward the removed one by the
 * share of removed lines, at the tile's heat. Files with no line changes
 * (pure renames) are [neutral].
 */
fun tileColor(heat: TileHeat, added: Color, removed: Color, neutral: Color): Color {
    val alpha = heat.alpha.coerceIn(0f, 1f)
    val ratio = heat.removedRatio ?: return neutral.copy(alpha = alpha)
    return lerp(added, removed, ratio.coerceIn(0f, 1f)).copy(alpha = alpha)
}

/** A tile's spoken description: `overview.rs in crates/rostrum/src/detail, 320 added`. */
fun tileDescription(tile: MapTile, directory: String): String {
    val changes = buildList {
        if (tile.additions > 0) add("${tile.additions} added")
        if (tile.deletions > 0) add("${tile.deletions} removed")
    }.ifEmpty { listOf("no line changes") }
    val where = if (directory.isEmpty()) "" else " in $directory"
    return "${tile.label}$where, ${changes.joinToString(", ")}"
}

/** Widths of the ranked row's green and red bar parts. */
data class BarWidths(val additions: Float, val deletions: Float)

/**
 * The stacked bar of a "largest changes" row, [width] wide: each part in
 * proportion to the largest file's churn, but never invisible when the file
 * has lines of that kind (2 for additions, 1 for deletions, as the mockup).
 */
fun rankedBar(file: RankedFile, width: Float): BarWidths {
    val add = if (file.additions > 0) (file.additionsShare.coerceIn(0f, 1f) * width).coerceAtLeast(2f) else 0f
    val del = if (file.deletions > 0) (file.deletionsShare.coerceIn(0f, 1f) * width).coerceAtLeast(1f) else 0f
    val total = add + del
    return if (total <= width || total == 0f) BarWidths(add, del) else BarWidths(add * width / total, del * width / total)
}

/** The status chip of the diff's sub-bar. */
data class StatusChipSpec(val text: String, val role: ColorRole)

fun statusChip(status: FileStatus): StatusChipSpec = when (status) {
    FileStatus.Added -> StatusChipSpec("Added", ColorRole.Success)
    FileStatus.Removed -> StatusChipSpec("Removed", ColorRole.Danger)
    FileStatus.Modified -> StatusChipSpec("Modified", ColorRole.Accent)
    FileStatus.Renamed -> StatusChipSpec("Renamed", ColorRole.Accent)
    FileStatus.Copied -> StatusChipSpec("Copied", ColorRole.Accent)
    FileStatus.Changed -> StatusChipSpec("Changed", ColorRole.Neutral)
    FileStatus.Unchanged -> StatusChipSpec("Unchanged", ColorRole.Neutral)
}

/** `3 added · 4 modified`, listing only the kinds present. */
fun summaryLine(stats: DiffStats): String = buildList {
    if (stats.addedFiles > 0) add("${stats.addedFiles} added")
    if (stats.modifiedFiles > 0) add("${stats.modifiedFiles} modified")
    if (stats.removedFiles > 0) add("${stats.removedFiles} removed")
    if (stats.renamedFiles > 0) add("${stats.renamedFiles} renamed")
}.joinToString(" · ")
