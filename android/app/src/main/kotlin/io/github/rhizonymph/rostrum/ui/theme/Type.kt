package io.github.rhizonymph.rostrum.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.R

/** Bundled typefaces (res/font; licenses in assets/licenses). */
object RostrumFonts {
    /** IBM Plex Sans: all UI text. */
    val Sans = FontFamily(
        Font(R.font.ibm_plex_sans_regular, FontWeight.Normal),
        Font(R.font.ibm_plex_sans_medium, FontWeight.Medium),
        Font(R.font.ibm_plex_sans_semibold, FontWeight.SemiBold),
    )

    /** JetBrains Mono: code, diffs, hashes, counts and other numbers. */
    val Mono = FontFamily(
        Font(R.font.jetbrains_mono_regular, FontWeight.Normal),
        Font(R.font.jetbrains_mono_medium, FontWeight.Medium),
        Font(R.font.jetbrains_mono_semibold, FontWeight.SemiBold),
    )
}

/** Material's type scale, set in [RostrumFonts.Sans]. */
val RostrumTypography: Typography = Typography().let { base ->
    fun TextStyle.sans() = copy(fontFamily = RostrumFonts.Sans)
    Typography(
        displayLarge = base.displayLarge.sans(),
        displayMedium = base.displayMedium.sans(),
        displaySmall = base.displaySmall.sans(),
        headlineLarge = base.headlineLarge.sans(),
        headlineMedium = base.headlineMedium.sans(),
        headlineSmall = base.headlineSmall.sans(),
        titleLarge = base.titleLarge.sans(),
        titleMedium = base.titleMedium.sans(),
        titleSmall = base.titleSmall.sans(),
        bodyLarge = base.bodyLarge.sans(),
        bodyMedium = base.bodyMedium.sans(),
        bodySmall = base.bodySmall.sans(),
        labelLarge = base.labelLarge.sans(),
        labelMedium = base.labelMedium.sans(),
        labelSmall = base.labelSmall.sans(),
    )
}

/** Monospace styles, read through [RostrumTheme.mono]. */
@Immutable
data class RostrumMonoTypography(
    /** Diff lines and code blocks. */
    val code: TextStyle,
    /** Emphasised code: hunk headers, file paths. */
    val codeStrong: TextStyle,
    /** Counts, line numbers, short hashes. */
    val number: TextStyle,
)

val DefaultRostrumMonoTypography = RostrumMonoTypography(
    code = TextStyle(fontFamily = RostrumFonts.Mono, fontWeight = FontWeight.Normal, fontSize = 13.sp, lineHeight = 20.sp),
    codeStrong = TextStyle(fontFamily = RostrumFonts.Mono, fontWeight = FontWeight.SemiBold, fontSize = 13.sp, lineHeight = 20.sp),
    number = TextStyle(fontFamily = RostrumFonts.Mono, fontWeight = FontWeight.Medium, fontSize = 12.sp, lineHeight = 16.sp),
)

val LocalRostrumMonoTypography = staticCompositionLocalOf { DefaultRostrumMonoTypography }
