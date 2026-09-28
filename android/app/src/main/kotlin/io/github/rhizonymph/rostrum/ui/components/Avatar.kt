package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.ui.format.initials
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts

/** The avatar tints of the mockups: fill, then initials colour. */
private val AvatarPalette = listOf(
    Color(0xFF25304A) to Color(0xFF8CBCFF),
    Color(0x2E3FB950) to Color(0xFF56D364),
    Color(0x2ED29922) to Color(0xFFE3B341),
    Color(0x2EBC8CFF) to Color(0xFFC8A8FF),
    Color(0x26F85149) to Color(0xFFFF7B72),
    Color(0xFF1D3A34) to Color(0xFFA6F0DC),
)

/** Stable per login, so a person keeps their colour everywhere. */
fun avatarColors(login: String): Pair<Color, Color> =
    AvatarPalette[Math.floorMod(login.lowercase().hashCode(), AvatarPalette.size)]

/**
 * A circle with the login's initials (no network images). Decorative: the
 * login is always shown as text next to it, so it has no content description.
 */
@Composable
fun Avatar(login: String, modifier: Modifier = Modifier, size: Dp = 24.dp) {
    val (fill, ink) = avatarColors(login)
    Box(
        modifier = modifier.size(size).clip(CircleShape).background(fill),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = initials(login),
            color = ink,
            fontFamily = RostrumFonts.Sans,
            fontWeight = FontWeight.SemiBold,
            fontSize = (size.value * 0.4f).coerceAtLeast(9f).sp,
            maxLines = 1,
        )
    }
}
