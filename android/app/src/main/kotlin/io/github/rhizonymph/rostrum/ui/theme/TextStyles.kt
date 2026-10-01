package io.github.rhizonymph.rostrum.ui.theme

import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

/**
 * The mockups' recurring text styles. Colours are applied at the call site
 * (from [RostrumTheme.colors]); these fix family, size, weight and leading.
 */
object RostrumText {
    private fun sans(size: Int, weight: FontWeight = FontWeight.Normal, line: Int? = null) = TextStyle(
        fontFamily = RostrumFonts.Sans,
        fontWeight = weight,
        fontSize = size.sp,
        lineHeight = (line ?: (size + 6)).sp,
    )

    private fun mono(size: Float, weight: FontWeight = FontWeight.Normal, line: Int? = null) = TextStyle(
        fontFamily = RostrumFonts.Mono,
        fontWeight = weight,
        fontSize = size.sp,
        lineHeight = (line ?: (size + 5).toInt()).sp,
    )

    /** "rostrum", "Settings", "Desktop": 22sp semibold, tight tracking. */
    val screenTitle = sans(22, FontWeight.SemiBold, 28).copy(letterSpacing = (-0.02).em)

    /** The sign-in wordmark. */
    val wordmark = sans(32, FontWeight.SemiBold, 40).copy(letterSpacing = (-0.02).em)

    /** Sheet and PR titles: 20sp semibold. */
    val sheetTitle = sans(20, FontWeight.SemiBold, 26)

    /** Feed row titles and list item titles: 15sp medium. */
    val rowTitle = sans(15, FontWeight.Medium, 20)

    /** Card headings such as the machine name: 16sp semibold. */
    val cardTitle = sans(16, FontWeight.SemiBold, 22)

    /** Section labels above cards: 13sp semibold, slightly tracked. */
    val section = sans(13, FontWeight.SemiBold, 17).copy(letterSpacing = 0.02.em)

    /** Markdown and comment bodies. */
    val body = sans(14, FontWeight.Normal, 21)

    /** Secondary lines: "#10 · ada-lin · 2h". */
    val meta = sans(13, FontWeight.Normal, 18)

    /** Captions and footnotes: 12sp. */
    val caption = sans(12, FontWeight.Normal, 17)

    /** Chip text: 12sp medium. */
    val chip = sans(12, FontWeight.Medium, 16)

    /** Button labels: 14sp semibold. */
    val button = sans(14, FontWeight.SemiBold, 20)

    /** Tab labels and medium buttons: 14sp medium. */
    val label = sans(14, FontWeight.Medium, 20)

    /** Navigation bar labels: 12sp. */
    val navLabel = sans(12, FontWeight.Medium, 16)

    /** Monospace: numbers, refs, SHAs, paths. */
    val mono12 = mono(12f)
    val mono11 = mono(11f)
    val mono13 = mono(13f)
    val monoStrong13 = mono(13f, FontWeight.SemiBold)
    val monoStrong15 = mono(15f, FontWeight.SemiBold, 20)
    val monoNumber16 = mono(16f, FontWeight.SemiBold, 22)
    val monoBig28 = mono(28f, FontWeight.SemiBold, 32)

    /** Diff lines: 12sp on a 20sp grid. */
    val diffLine = mono(12f, FontWeight.Normal, 20)
    val diffGutter = mono(11f, FontWeight.Normal, 20)
}
