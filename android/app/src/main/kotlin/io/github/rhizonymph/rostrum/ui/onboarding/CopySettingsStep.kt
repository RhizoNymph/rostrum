package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.desktopconfig.DesktopConfigPreviewView
import io.github.rhizonymph.rostrum.ui.desktopconfig.DesktopConfigText
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The question after pairing a desktop into a new profile: take the
 * desktop's repositories and feed settings, or keep the profile's. Shown
 * only when copying would change something.
 */
@Composable
fun CopySettingsStep(offer: CopyOffer, actions: PairActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val machine = offer.preview.machine
    Column(modifier.fillMaxSize().background(colors.bg).safeDrawingPadding()) {
        Column(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                DesktopConfigText.offerTitle(machine),
                style = RostrumText.sheetTitle,
                color = colors.text,
                modifier = Modifier.semantics { heading() },
            )
            Text(
                "Paired with $machine. Use its repositories and feed settings in this profile too? " +
                    "You can do this later from Settings.",
                style = RostrumText.body,
                color = colors.textSecondary,
            )
            DesktopConfigPreviewView(offer.preview, changes = offer.changes)
        }
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            (offer.action as? ActionState.Failed)?.let { FieldError("Couldn't copy: ${it.error.describe()}") }
            PrimaryButton(
                "Copy settings",
                actions::copySettings,
                modifier = Modifier.fillMaxWidth(),
                busy = offer.action.running,
                height = 48.dp,
            )
            TextPillButton("Not now", actions::keepPhoneSettings, enabled = !offer.action.running)
        }
    }
}

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun CopySettingsStepPreview() {
    RostrumTheme {
        CopySettingsStep(
            CopyOffer(
                ProfileId.of("preview")!!,
                DesktopConfigPreview(
                    machine = "framework",
                    repos = listOf("RhizoNymph/rostrum", "zed-industries/zed", "serde-rs/serde"),
                    added = listOf("serde-rs/serde"),
                    removed = listOf("rust-lang/rust"),
                    prsPerRepo = 25,
                    hideDrafts = true,
                    hideEmptyRepos = true,
                    authors = emptyList(),
                    includeInvolved = false,
                    autostash = true,
                    changesAnything = true,
                ),
            ),
            NoPairActions,
        )
    }
}
