package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.CheckState

/** What a CI glyph shows. */
enum class CiShape { Passing, Failing, Running, None, Skipped }

fun CheckState?.ciShape(): CiShape = when (this) {
    CheckState.Success -> CiShape.Passing
    CheckState.Failure, CheckState.Error -> CiShape.Failing
    CheckState.Pending, CheckState.Expected -> CiShape.Running
    null -> CiShape.None
}

fun CiShape.describe(): String = when (this) {
    CiShape.Passing -> "Checks passing"
    CiShape.Failing -> "Checks failing"
    CiShape.Running -> "Checks running"
    CiShape.None -> "No checks"
    CiShape.Skipped -> "Skipped"
}

/**
 * The 18dp CI state glyph of the feed and checks list: a ring with a check,
 * a cross, a centre dot, or dashed (nothing reported / skipped).
 */
@Composable
fun CiGlyph(
    shape: CiShape,
    color: Color,
    modifier: Modifier = Modifier,
    size: Dp = 18.dp,
    strokeWidth: Float = 2f,
    contentDescription: String? = shape.describe(),
) {
    val check = remember { PathParser().parsePathString("M8 12.5l2.7 2.7L16 10").toPath() }
    val cross = remember { PathParser().parsePathString("M9.2 9.2l5.6 5.6M14.8 9.2l-5.6 5.6").toPath() }
    val dash = remember { PathParser().parsePathString("M8.5 12h7").toPath() }
    val semantics = if (contentDescription != null) {
        Modifier.semantics { this.contentDescription = contentDescription }
    } else {
        Modifier
    }
    Canvas(modifier.size(size).then(semantics)) {
        scale(this.size.width / 24f, pivot = Offset.Zero) {
            val stroke = Stroke(width = strokeWidth, cap = StrokeCap.Round, join = StrokeJoin.Round)
            val dashed = Stroke(
                width = strokeWidth, cap = StrokeCap.Round,
                pathEffect = PathEffect.dashPathEffect(floatArrayOf(3.5f, 3f)),
            )
            when (shape) {
                CiShape.Passing -> {
                    drawCircle(color, radius = 9f, center = Offset(12f, 12f), style = stroke)
                    drawPath(check, color, style = stroke)
                }
                CiShape.Failing -> {
                    drawCircle(color, radius = 9f, center = Offset(12f, 12f), style = stroke)
                    drawPath(cross, color, style = stroke)
                }
                CiShape.Running -> {
                    drawCircle(color, radius = 9f, center = Offset(12f, 12f), style = stroke)
                    drawCircle(color, radius = 3f, center = Offset(12f, 12f))
                }
                CiShape.None -> drawCircle(color, radius = 9f, center = Offset(12f, 12f), style = dashed)
                CiShape.Skipped -> {
                    drawCircle(color, radius = 9f, center = Offset(12f, 12f), style = dashed)
                    drawPath(dash, color, style = stroke)
                }
            }
        }
    }
}
