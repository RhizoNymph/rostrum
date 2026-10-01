package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.ui.theme.RostrumColors
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * What a [ColorRole] paints: [text] for chip text and coloured numbers, [tint]
 * for chip and banner fills, [solid] for dots, glyphs and bars.
 */
@Immutable
data class RoleColors(val text: Color, val tint: Color, val solid: Color)

/** The draft grey from the core's reference palette (`#8b949e`). */
val DraftSolid = Color(0xFF8B949E)

fun ColorRole.colors(palette: RostrumColors): RoleColors = when (this) {
    ColorRole.Success -> RoleColors(palette.successText, palette.success.copy(alpha = 0.15f), palette.success)
    ColorRole.Warning -> RoleColors(palette.warningText, palette.warning.copy(alpha = 0.15f), palette.warning)
    ColorRole.Danger -> RoleColors(palette.dangerText, palette.danger.copy(alpha = 0.15f), palette.danger)
    ColorRole.Draft -> RoleColors(palette.draftText, DraftSolid.copy(alpha = 0.18f), DraftSolid)
    ColorRole.Accent -> RoleColors(palette.accentText, palette.accent.copy(alpha = 0.15f), palette.accent)
    ColorRole.Neutral -> RoleColors(palette.textMuted, palette.raised, palette.textSubtle)
}

@Composable
@ReadOnlyComposable
fun ColorRole.colors(): RoleColors = colors(RostrumTheme.colors)

/** A GitHub label colour (opaque ARGB) as chip colours legible on the dark palette. */
fun labelColors(argb: Int?, palette: RostrumColors): RoleColors {
    if (argb == null) return ColorRole.Neutral.colors(palette)
    val base = Color(argb)
    return RoleColors(
        text = lerp(base, Color.White, 0.45f),
        tint = base.copy(alpha = 0.18f),
        solid = base,
    )
}
