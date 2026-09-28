package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

val CardShape = RoundedCornerShape(16.dp)

/** The mockups' card: surface fill, 1dp border, radius 16, clipped. */
@Composable
fun RostrumCard(
    modifier: Modifier = Modifier,
    color: Color = RostrumTheme.colors.surface,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier = modifier
            .clip(CardShape)
            .background(color)
            .border(1.dp, RostrumTheme.colors.border, CardShape),
        content = content,
    )
}

/** The 1dp rule between rows inside a card. */
@Composable
fun CardDivider(modifier: Modifier = Modifier, inset: Dp = 0.dp) {
    Box(
        modifier
            .padding(horizontal = inset)
            .fillMaxWidth()
            .height(1.dp)
            .background(RostrumTheme.colors.border),
    )
}

/** "Repositories 5", "Handoff sessions 1 waiting": the label above a card. */
@Composable
fun SectionHeader(
    text: String,
    modifier: Modifier = Modifier,
    trailing: String? = null,
    trailingColor: Color = RostrumTheme.colors.textSubtle,
    end: (@Composable RowScope.() -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier.padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, style = RostrumText.section, color = colors.textMuted, modifier = Modifier.semantics { heading() })
        if (trailing != null) {
            Spacer(Modifier.width(8.dp))
            Text(trailing, style = RostrumText.mono12, color = trailingColor)
        }
        if (end != null) {
            Spacer(Modifier.weight(1f))
            end()
        }
    }
}

/** A small filled circle: connection state, merge verdict, conflicted file. */
@Composable
fun StatusDot(color: Color, modifier: Modifier = Modifier, size: Dp = 7.dp) {
    Box(modifier.size(size).clip(CircleShape).background(color))
}

/** A rounded square holding an icon: the machine tile, the handoff glyph. */
@Composable
fun IconTile(
    modifier: Modifier = Modifier,
    size: Dp = 36.dp,
    radius: Dp = 10.dp,
    color: Color = RostrumTheme.colors.tonal,
    content: @Composable () -> Unit,
) {
    Box(
        modifier = modifier.size(size).clip(RoundedCornerShape(radius)).background(color),
        contentAlignment = Alignment.Center,
    ) { content() }
}
