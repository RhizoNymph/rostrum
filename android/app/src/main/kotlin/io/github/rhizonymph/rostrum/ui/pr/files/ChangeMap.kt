package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.data.model.FilesOverview
import io.github.rhizonymph.rostrum.data.model.MapColumn
import io.github.rhizonymph.rostrum.data.model.MapTile
import io.github.rhizonymph.rostrum.ui.components.CardShape
import io.github.rhizonymph.rostrum.ui.format.additionsText
import io.github.rhizonymph.rostrum.ui.format.deletionsText
import io.github.rhizonymph.rostrum.ui.format.directoryOf
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

private val TileInk = Color(0xFFF0F3F8)
private val TileAdded = Color(0xFFD7F5DC)
private val TileRemoved = Color(0xFFFFA198)

/**
 * The change map: a column per directory (width = its share of the churn), a
 * tile per file (height = its share of the column), coloured by how much of
 * the file's change is removal. Tapping a file's tile opens its diff.
 */
@Composable
fun ChangeMapCard(overview: FilesOverview, onOpenFile: (Int) -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    BoxWithConstraints(
        modifier = modifier
            .fillMaxWidth()
            .height(ChangeMapMetrics.HEIGHT.dp)
            .clip(CardShape)
            .background(colors.surface)
            .border(1.dp, colors.border, CardShape)
            .padding(ChangeMapMetrics.PADDING.dp),
    ) {
        val widths = distribute(
            shares = overview.changeMap.map { it.share },
            available = maxWidth.value,
            gap = ChangeMapMetrics.COLUMN_GAP,
            min = ChangeMapMetrics.MIN_COLUMN,
        )
        val directories = overview.files.associate { it.index to directoryOf(it.path).removeSuffix("/") }
        Row(horizontalArrangement = Arrangement.spacedBy(ChangeMapMetrics.COLUMN_GAP.dp)) {
            overview.changeMap.forEachIndexed { index, column ->
                MapColumnView(column, widths[index], directories, onOpenFile)
            }
        }
    }
}

@Composable
private fun MapColumnView(
    column: MapColumn,
    width: Float,
    directories: Map<Int, String>,
    onOpenFile: (Int) -> Unit,
) {
    val colors = RostrumTheme.colors
    Column(Modifier.width(width.dp), verticalArrangement = Arrangement.spacedBy(ChangeMapMetrics.LABEL_GAP.dp)) {
        Box(Modifier.fillMaxWidth().height(ChangeMapMetrics.LABEL_HEIGHT.dp)) {
            if (showsColumnLabel(width)) {
                Text(
                    column.label,
                    style = RostrumText.mono11.copy(fontSize = 10.sp, lineHeight = 14.sp),
                    color = colors.textMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) {
            val heights = distribute(
                shares = column.tiles.map { it.share },
                available = maxHeight.value,
                gap = ChangeMapMetrics.TILE_GAP,
                min = ChangeMapMetrics.MIN_TILE,
            )
            Column(verticalArrangement = Arrangement.spacedBy(ChangeMapMetrics.TILE_GAP.dp)) {
                column.tiles.forEachIndexed { index, tile ->
                    val directory = tile.fileIndex?.let { directories[it] } ?: column.label
                    MapTileView(tile, width, heights[index], directory, onOpenFile)
                }
            }
        }
    }
}

@Composable
private fun MapTileView(tile: MapTile, width: Float, height: Float, directory: String, onOpenFile: (Int) -> Unit) {
    val colors = RostrumTheme.colors
    val fill = tileColor(tile.heat, colors.success, colors.danger, colors.textSubtle)
    val shape = RoundedCornerShape(if (width < 20f || height < 20f) 3.dp else 6.dp)
    val description = tileDescription(tile, directory)
    val fileIndex = tile.fileIndex
    Box(
        modifier = Modifier
            .size(width.dp, height.dp)
            .clip(shape)
            .background(fill)
            .then(
                if (fileIndex != null) {
                    Modifier.clickable(role = Role.Button, onClickLabel = "Open diff") { onOpenFile(fileIndex) }
                } else {
                    Modifier
                },
            )
            .semantics { contentDescription = description },
    ) {
        when (tileContent(width, height)) {
            TileContent.Stacked -> Column(
                Modifier.padding(horizontal = 8.dp, vertical = 7.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                TileName(tile.label)
                TileCounts(tile)
            }
            TileContent.Inline -> Row(
                Modifier.fillMaxSize().padding(horizontal = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                TileName(tile.label, Modifier.weight(1f, fill = false))
                TileCounts(tile)
            }
            TileContent.Vertical -> Box(
                Modifier.fillMaxSize().padding(vertical = 8.dp),
                contentAlignment = Alignment.TopCenter,
            ) {
                Text(
                    text = buildAnnotatedString {
                        withStyle(RostrumText.mono11.copy(fontWeight = FontWeight.SemiBold, color = TileInk).toSpanStyle()) {
                            append(tile.label)
                        }
                        if (tile.additions > 0) {
                            withStyle(RostrumText.mono11.copy(color = TileAdded).toSpanStyle()) {
                                append(" " + additionsText(tile.additions))
                            }
                        }
                    },
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.rotatedVertically(),
                )
            }
            TileContent.None -> Spacer(Modifier)
        }
    }
}

@Composable
private fun TileName(name: String, modifier: Modifier = Modifier) {
    Text(
        name,
        style = RostrumText.mono11.copy(fontWeight = FontWeight.SemiBold),
        color = TileInk,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier,
    )
}

@Composable
private fun TileCounts(tile: MapTile) {
    Text(
        buildAnnotatedString {
            if (tile.additions > 0 || tile.deletions == 0L) {
                withStyle(RostrumText.mono11.copy(color = TileAdded).toSpanStyle()) { append(additionsText(tile.additions)) }
            }
            if (tile.deletions > 0) {
                if (tile.additions > 0) append(" ")
                withStyle(RostrumText.mono11.copy(color = TileRemoved).toSpanStyle()) { append(deletionsText(tile.deletions)) }
            }
        },
        style = RostrumText.mono11.copy(fontFamily = RostrumFonts.Mono),
        maxLines = 1,
    )
}

/** Lay the text out along the tile's height, reading top to bottom. */
private fun Modifier.rotatedVertically(): Modifier = layout { measurable, constraints ->
    val placeable = measurable.measure(
        constraints.copy(minWidth = 0, maxWidth = constraints.maxHeight, minHeight = 0, maxHeight = constraints.maxWidth),
    )
    layout(placeable.height, placeable.width) {
        placeable.placeWithLayer(
            x = (placeable.height - placeable.width) / 2,
            y = (placeable.width - placeable.height) / 2,
        ) { rotationZ = 90f }
    }
}
