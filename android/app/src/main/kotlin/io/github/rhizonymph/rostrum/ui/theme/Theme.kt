package io.github.rhizonymph.rostrum.ui.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver

/**
 * Provides [RostrumColors], [RostrumMonoTypography], and a Material 3 theme
 * derived from them. Always dark.
 */
@Composable
fun RostrumTheme(content: @Composable () -> Unit) {
    val colors = DarkRostrumColors
    CompositionLocalProvider(
        LocalRostrumColors provides colors,
        LocalRostrumMonoTypography provides DefaultRostrumMonoTypography,
    ) {
        MaterialTheme(
            colorScheme = colors.toMaterialColorScheme(),
            typography = RostrumTypography,
            content = content,
        )
    }
}

/** Accessors for the Rostrum-specific theme values, like [MaterialTheme]'s. */
object RostrumTheme {
    val colors: RostrumColors
        @Composable @ReadOnlyComposable
        get() = LocalRostrumColors.current

    val mono: RostrumMonoTypography
        @Composable @ReadOnlyComposable
        get() = LocalRostrumMonoTypography.current
}

/**
 * Maps the palette onto Material roles, so stock components (buttons, fields,
 * dialogs, menus) look native to the app without per-call colour overrides.
 * The container tints for error/tertiary are not palette entries; they are the
 * status colour at low opacity over [RostrumColors.surface].
 */
fun RostrumColors.toMaterialColorScheme(): ColorScheme = darkColorScheme(
    primary = accent,
    onPrimary = onAccent,
    primaryContainer = tonal,
    onPrimaryContainer = onTonal,
    inversePrimary = accentText,
    secondary = accentText,
    onSecondary = onAccent,
    secondaryContainer = tonal,
    onSecondaryContainer = onTonal,
    tertiary = success,
    onTertiary = onMerge,
    tertiaryContainer = success.copy(alpha = 0.16f).compositeOver(surface),
    onTertiaryContainer = successText,
    background = bg,
    onBackground = text,
    surface = surface,
    onSurface = text,
    surfaceVariant = raised,
    onSurfaceVariant = textMuted,
    surfaceTint = accent,
    inverseSurface = text,
    inverseOnSurface = bg,
    error = danger,
    onError = bg,
    errorContainer = danger.copy(alpha = 0.16f).compositeOver(surface),
    onErrorContainer = dangerText,
    outline = borderStrong,
    outlineVariant = border,
    scrim = Color.Black,
    surfaceBright = raised,
    surfaceDim = bg,
    surfaceContainerLowest = bg,
    surfaceContainerLow = surface,
    surfaceContainer = surface,
    surfaceContainerHigh = raised,
    surfaceContainerHighest = raised,
)
