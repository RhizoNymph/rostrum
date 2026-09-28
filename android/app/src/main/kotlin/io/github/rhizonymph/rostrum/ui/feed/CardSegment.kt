package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp

/**
 * Where a lazy item sits inside a repository's card. The card is drawn as
 * segments so a repository with many pull requests stays lazily composed:
 * the header rounds the top, the last row the bottom, and every segment
 * draws its sides and its bottom rule (the divider under it).
 */
enum class SegmentPosition {
    Single, Top, Middle, Bottom;

    val shape: Shape
        get() = when (this) {
            Single -> RoundedCornerShape(16.dp)
            Top -> RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp)
            Middle -> RectangleShape
            Bottom -> RoundedCornerShape(bottomStart = 16.dp, bottomEnd = 16.dp)
        }

    companion object {
        /** The position of item [index] of [count] segments. */
        fun of(index: Int, count: Int): SegmentPosition = when {
            count <= 1 -> Single
            index == 0 -> Top
            index == count - 1 -> Bottom
            else -> Middle
        }
    }
}

/** Card fill, clip and 1dp outline for one segment of a card. */
fun Modifier.cardSegment(position: SegmentPosition, fill: Color, stroke: Color): Modifier = this
    .clip(position.shape)
    .background(fill)
    .drawWithContent {
        drawContent()
        val w = 1.dp.toPx()
        val r = 16.dp.toPx()
        val half = w / 2
        val right = size.width - half
        val bottom = size.height - half
        val topRounded = position == SegmentPosition.Single || position == SegmentPosition.Top
        val bottomRounded = position == SegmentPosition.Single || position == SegmentPosition.Bottom
        val path = Path().apply {
            // Left side, bottom to top.
            if (bottomRounded) {
                moveTo(half + r, bottom)
                arcTo(Rect(Offset(half, bottom - 2 * r), Size(2 * r, 2 * r)), 90f, 90f, false)
            } else {
                moveTo(half, bottom)
            }
            if (topRounded) {
                lineTo(half, half + r)
                arcTo(Rect(Offset(half, half), Size(2 * r, 2 * r)), 180f, 90f, false)
                lineTo(right - r, half)
                arcTo(Rect(Offset(right - 2 * r, half), Size(2 * r, 2 * r)), 270f, 90f, false)
            } else {
                lineTo(half, 0f)
                moveTo(right, 0f)
            }
            if (bottomRounded) {
                lineTo(right, bottom - r)
                arcTo(Rect(Offset(right - 2 * r, bottom - 2 * r), Size(2 * r, 2 * r)), 0f, 90f, false)
                lineTo(half + r, bottom)
            } else {
                lineTo(right, bottom)
                lineTo(half, bottom)
            }
        }
        drawPath(path, stroke, style = Stroke(width = w))
    }
