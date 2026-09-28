package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The first-run screens' larger card: radius 20, padding 20. */
@Composable
internal fun OnboardingCard(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(20.dp)
    Column(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface)
            .border(1.dp, colors.border, shape)
            .padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
        content = content,
    )
}

/** A warning-tinted banner, e.g. why the app signed out by itself. */
@Composable
internal fun NoticeBanner(text: String, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Row(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(colors.warning.copy(alpha = 0.12f))
            .padding(12.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.warningText, modifier = Modifier.size(18.dp))
        Text(text, style = RostrumText.meta, color = colors.textSecondary)
    }
}

/** A line with a leading check, as in "What pairing allows". */
@Composable
internal fun CheckLine(text: String) {
    val colors = RostrumTheme.colors
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
        Icon(
            RostrumIcons.Check,
            contentDescription = null,
            tint = colors.successText,
            modifier = Modifier.padding(top = 1.dp).size(18.dp),
        )
        Text(text, style = RostrumText.label.copy(fontWeight = RostrumText.body.fontWeight), color = colors.text)
    }
}
