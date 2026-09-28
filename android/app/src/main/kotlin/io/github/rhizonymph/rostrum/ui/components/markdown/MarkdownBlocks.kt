package io.github.rhizonymph.rostrum.ui.components.markdown

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.MdSpan
import io.github.rhizonymph.rostrum.ui.theme.RostrumColors
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * Renders the core's flat markdown blocks: paragraphs, headings, code, list
 * items (with nesting as indentation), quotes (as bars), rules, tables (runs
 * of TableRow blocks) and images (as their alt text, linked). Links open in
 * the browser through [LinkAnnotation.Url].
 */
@Composable
fun MarkdownBlocks(
    blocks: List<MdBlock>,
    modifier: Modifier = Modifier,
    textStyle: TextStyle = RostrumText.body,
    textColor: Color = RostrumTheme.colors.textSecondary,
) {
    val colors = RostrumTheme.colors
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        var index = 0
        while (index < blocks.size) {
            val block = blocks[index]
            if (block.kind is MdBlockKind.TableRow) {
                val rows = blocks.drop(index).takeWhile { it.kind is MdBlockKind.TableRow }
                Quoted(block.quoteDepth, colors) { MdTable(rows.map { it.kind as MdBlockKind.TableRow }, textStyle) }
                index += rows.size
                continue
            }
            Quoted(block.quoteDepth, colors) {
                Block(block, textStyle, textColor, colors)
            }
            index++
        }
    }
}

@Composable
private fun Quoted(depth: Int, colors: RostrumColors, content: @Composable () -> Unit) {
    if (depth == 0) {
        content()
        return
    }
    Row(Modifier.height(IntrinsicSize.Min)) {
        repeat(depth) {
            Box(Modifier.padding(end = 10.dp).width(3.dp).fillMaxHeight().background(colors.borderStrong))
        }
        Box(Modifier.weight(1f)) { content() }
    }
}

@Composable
private fun Block(block: MdBlock, style: TextStyle, color: Color, colors: RostrumColors) {
    val indent = (block.listDepth - 1).coerceAtLeast(0) * 18
    when (val kind = block.kind) {
        MdBlockKind.Paragraph -> Text(
            spansToAnnotated(block.spans, colors),
            style = style,
            color = color,
            modifier = Modifier.padding(start = if (block.listDepth > 0) (indent + 18).dp else 0.dp),
        )
        is MdBlockKind.Heading -> Text(
            spansToAnnotated(block.spans, colors),
            style = style.copy(
                fontSize = when (kind.level) { 1 -> 18.sp; 2 -> 16.sp; else -> 15.sp },
                fontWeight = FontWeight.SemiBold,
                lineHeight = 24.sp,
            ),
            color = colors.text,
        )
        is MdBlockKind.Code -> {
            val shape = RoundedCornerShape(8.dp)
            Box(
                Modifier
                    .fillMaxWidth()
                    .background(colors.bg, shape)
                    .border(1.dp, colors.border, shape)
                    .horizontalScroll(rememberScrollState())
                    .padding(10.dp),
            ) {
                Text(kind.code, style = RostrumText.mono12, color = colors.text, softWrap = false)
            }
        }
        is MdBlockKind.ListItem -> Row(Modifier.padding(start = indent.dp)) {
            val marker = when {
                kind.checked == true -> "☑"
                kind.checked == false -> "☐"
                kind.ordered -> "${kind.number}."
                else -> "•"
            }
            Text(marker, style = style, color = colors.textMuted, modifier = Modifier.widthIn(min = 18.dp))
            Text(spansToAnnotated(block.spans, colors), style = style, color = color)
        }
        MdBlockKind.Rule -> Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
        is MdBlockKind.Image -> Text(
            spansToAnnotated(listOf(MdSpan(text = kind.alt.ifBlank { "image" }, link = kind.url)), colors),
            style = style,
            color = color,
        )
        is MdBlockKind.TableRow -> Unit
    }
}

@Composable
private fun MdTable(rows: List<MdBlockKind.TableRow>, style: TextStyle) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(8.dp)
    Column(
        Modifier
            .border(1.dp, colors.border, shape)
            .horizontalScroll(rememberScrollState()),
    ) {
        rows.forEachIndexed { rowIndex, row ->
            if (rowIndex > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
            Row {
                row.cells.forEach { cell ->
                    Text(
                        spansToAnnotated(cell, colors),
                        style = style.copy(fontWeight = if (row.header) FontWeight.SemiBold else style.fontWeight),
                        color = if (row.header) colors.text else colors.textSecondary,
                        modifier = Modifier.widthIn(min = 72.dp, max = 220.dp).padding(horizontal = 10.dp, vertical = 6.dp),
                    )
                }
            }
        }
    }
}

/** Inline spans to styled text: bold, italic, code (mono on the page colour), strike, links. */
fun spansToAnnotated(spans: List<MdSpan>, colors: RostrumColors): AnnotatedString = buildAnnotatedString {
    for (span in spans) {
        val style = SpanStyle(
            fontWeight = if (span.bold) FontWeight.Medium else null,
            fontStyle = if (span.italic) FontStyle.Italic else null,
            fontFamily = if (span.code) RostrumFonts.Mono else null,
            fontSize = if (span.code) 12.5.sp else androidx.compose.ui.unit.TextUnit.Unspecified,
            background = if (span.code) colors.bg else Color.Unspecified,
            color = if (span.bold || span.code) colors.text else Color.Unspecified,
            textDecoration = if (span.strike) TextDecoration.LineThrough else null,
        )
        val link = span.link
        if (link != null) {
            withLink(
                LinkAnnotation.Url(
                    link,
                    TextLinkStyles(style = style.merge(SpanStyle(color = colors.accentText))),
                ),
            ) { append(span.text) }
        } else {
            withStyle(style) { append(span.text) }
        }
    }
}
