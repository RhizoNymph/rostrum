package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.CodeSegment
import io.github.rhizonymph.rostrum.data.model.DiffLineView
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.ui.format.MINUS
import io.github.rhizonymph.rostrum.ui.theme.RostrumColors
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The line-number gutter and the +/− marker column, in dp. */
const val GUTTER_WIDTH = 36
const val MARKER_WIDTH = 16

/** Tabs are shown as four spaces, so widths measured from text agree with what is drawn. */
fun expandTabs(text: String): String = text.replace("\t", "    ")

/** A line's syntax runs as styled text; changed words get a stronger background. */
fun codeText(segments: List<CodeSegment>, emphasis: Color): AnnotatedString = buildAnnotatedString {
    for (segment in segments) {
        withStyle(
            SpanStyle(
                color = Color(segment.argb),
                fontWeight = if (segment.bold) FontWeight.SemiBold else null,
                fontStyle = if (segment.italic) FontStyle.Italic else null,
                background = if (segment.emphasized) emphasis else Color.Unspecified,
            ),
        ) { append(expandTabs(segment.text)) }
    }
}

private data class LineColors(val row: Color, val gutter: Color, val marker: Color, val emphasis: Color)

private fun lineColors(kind: LineKind, palette: RostrumColors): LineColors = when (kind) {
    LineKind.Added -> LineColors(
        row = palette.success.copy(alpha = 0.12f),
        gutter = palette.success.copy(alpha = 0.20f),
        marker = palette.successText,
        emphasis = palette.success.copy(alpha = 0.35f),
    )
    LineKind.Removed -> LineColors(
        row = palette.danger.copy(alpha = 0.12f),
        gutter = palette.danger.copy(alpha = 0.20f),
        marker = palette.dangerText,
        emphasis = palette.danger.copy(alpha = 0.35f),
    )
    LineKind.Context -> LineColors(Color.Transparent, Color.Transparent, palette.textSubtle, palette.accent.copy(alpha = 0.25f))
}

/**
 * One 20dp diff line: gutter number, marker, code. Without [softWrap] the code
 * is laid out unwrapped and shifted by the shared horizontal offset
 * ([scrollX], read at draw time so scrolling never recomposes rows).
 * Commenting is started from the list's gesture handler; the gutter carries
 * the equivalent accessibility action.
 */
@Composable
fun DiffLineRow(
    line: DiffLineView,
    selected: Boolean,
    softWrap: Boolean,
    scrollX: () -> Float,
    onComment: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val palette = RostrumTheme.colors
    val colors = lineColors(line.kind, palette)
    val selectedTint = palette.accent.copy(alpha = 0.22f)
    val code = remember(line, colors.emphasis) { codeText(line.segments, colors.emphasis) }
    val number = when (line.kind) {
        LineKind.Removed -> line.oldLine
        LineKind.Added, LineKind.Context -> line.newLine
    }
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 20.dp)
            .background(if (selected) selectedTint else colors.row)
            .drawBehind {
                drawRect(
                    color = if (selected) palette.accent.copy(alpha = 0.35f) else colors.gutter,
                    size = Size(GUTTER_WIDTH.dp.toPx(), size.height),
                )
            },
    ) {
        Text(
            text = number?.toString().orEmpty(),
            style = RostrumText.diffGutter,
            color = if (selected) palette.onTonal else palette.textMuted,
            textAlign = TextAlign.End,
            maxLines = 1,
            modifier = Modifier
                .width(GUTTER_WIDTH.dp)
                .padding(end = 6.dp)
                .then(
                    if (onComment != null && number != null) {
                        Modifier.semantics {
                            contentDescription = "Comment on line $number"
                            role = Role.Button
                            onClick(label = "Comment on line $number") {
                                onComment()
                                true
                            }
                        }
                    } else {
                        Modifier
                    },
                ),
        )
        Text(
            text = when (line.kind) {
                LineKind.Added -> "+"
                LineKind.Removed -> MINUS
                LineKind.Context -> " "
            },
            style = RostrumText.diffLine,
            color = colors.marker,
            textAlign = TextAlign.Center,
            modifier = Modifier.width(MARKER_WIDTH.dp),
        )
        Box(Modifier.weight(1f).clipToBounds()) {
            if (softWrap) {
                Text(code, style = RostrumText.diffLine, color = palette.text, modifier = Modifier.padding(end = 12.dp))
            } else {
                Text(
                    code,
                    style = RostrumText.diffLine,
                    color = palette.text,
                    softWrap = false,
                    maxLines = 1,
                    modifier = Modifier
                        .wrapContentWidth(Alignment.Start, unbounded = true)
                        .graphicsLayer { translationX = -scrollX() }
                        .padding(end = 12.dp),
                )
            }
        }
    }
}

/** A hunk's `@@ -a,b +c,d @@ context` line. */
@Composable
fun HunkHeaderRow(header: String, softWrap: Boolean, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Box(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 28.dp)
            .background(colors.accent.copy(alpha = 0.08f))
            .padding(horizontal = 12.dp, vertical = 4.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            header,
            style = RostrumText.mono12,
            color = colors.accentText,
            maxLines = if (softWrap) Int.MAX_VALUE else 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
