package io.github.rhizonymph.rostrum.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/**
 * Rostrum's palette, named by role. Screens read these through
 * [RostrumTheme.colors]; Material components get the same values through the
 * mapped [androidx.compose.material3.ColorScheme] (see `Theme.kt`).
 */
@Immutable
data class RostrumColors(
    /** Page background. */
    val bg: Color,
    /** Cards and panels on [bg]. */
    val surface: Color,
    /** Elements raised above [surface]: menus, sheets, inputs. */
    val raised: Color,
    /** Tonal/selected fill (selected row, active chip) and the content on it. */
    val tonal: Color,
    val onTonal: Color,
    val border: Color,
    val borderStrong: Color,
    /** Text, from most to least prominent. */
    val text: Color,
    val textSecondary: Color,
    val textMuted: Color,
    val textSubtle: Color,
    /** Primary action fill, the content on it, and accent-coloured text/links. */
    val accent: Color,
    val onAccent: Color,
    val accentText: Color,
    /** Status fills (icons, dots, bars) and their text variants. */
    val success: Color,
    val successText: Color,
    val warning: Color,
    val warningText: Color,
    val danger: Color,
    val dangerText: Color,
    /** Text of draft pull requests. */
    val draftText: Color,
    /** The merge button and the content on it. */
    val merge: Color,
    val onMerge: Color,
)

/** The only palette: Rostrum is dark-themed. */
val DarkRostrumColors = RostrumColors(
    bg = Color(0xFF0F1115),
    surface = Color(0xFF161920),
    raised = Color(0xFF1C2029),
    tonal = Color(0xFF25304A),
    onTonal = Color(0xFFD6E4FF),
    border = Color(0xFF272B36),
    borderStrong = Color(0xFF39404F),
    text = Color(0xFFE4E7EE),
    textSecondary = Color(0xFFCFD5E1),
    textMuted = Color(0xFF9AA2B4),
    textSubtle = Color(0xFF858DA0),
    accent = Color(0xFF5B9DFF),
    onAccent = Color(0xFF0B1220),
    accentText = Color(0xFF8CBCFF),
    success = Color(0xFF3FB950),
    successText = Color(0xFF56D364),
    warning = Color(0xFFD29922),
    warningText = Color(0xFFE3B341),
    danger = Color(0xFFF85149),
    dangerText = Color(0xFFFF7B72),
    draftText = Color(0xFFB1B8C3),
    merge = Color(0xFF3FB950),
    onMerge = Color(0xFF06120A),
)

val LocalRostrumColors = staticCompositionLocalOf { DarkRostrumColors }
