package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** After pairing a new desktop while another profile is in use: "Switch to <machine>?" */
@Composable
fun SwitchProfileStep(offer: SwitchOffer, actions: PairActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxSize().background(colors.bg).safeDrawingPadding()) {
        Column(
            Modifier.weight(1f).padding(horizontal = 20.dp, vertical = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                "Switch to ${offer.machine}?",
                style = RostrumText.sheetTitle,
                color = colors.text,
                modifier = Modifier.semantics { heading() },
            )
            Text(
                "Paired with ${offer.machine}. It has its own profile: its own repositories, filters, drafts and " +
                    "GitHub account. Switch to it now, or later from the profile menu in the feed's header.",
                style = RostrumText.body,
                color = colors.textSecondary,
            )
        }
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            PrimaryButton(
                "Switch to ${offer.machine}",
                actions::switchToPaired,
                modifier = Modifier.fillMaxWidth(),
                height = 48.dp,
            )
            TextPillButton(offer.current?.let { "Stay on $it" } ?: "Not now", actions::stayOnCurrent)
        }
    }
}
