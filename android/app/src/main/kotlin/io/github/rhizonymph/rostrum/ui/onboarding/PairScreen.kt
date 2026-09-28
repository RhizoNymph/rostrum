package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** Pair with desktop, stateless: link preview, instructions, manual entry. */
@Composable
fun PairScreen(
    state: PairUiState,
    actions: PairActions,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val offer = state.copy
    if (offer != null) {
        CopySettingsStep(offer, actions, modifier)
        return
    }
    Column(modifier.fillMaxSize().background(colors.bg).imePadding()) {
        BackTopBar(onBack = onBack) {
            Text("Pair with desktop", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.semantics { heading() })
        }
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 4.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            when (val link = state.link) {
                LinkState.Reading -> LoadingView(label = "Reading the pairing link…")
                is LinkState.Preview -> LinkPreviewSection(link.preview, link.pairing, actions::pairWithLink)
                is LinkState.Invalid -> ErrorView(link.error, title = "This pairing link can't be used")
                null -> PairInstructions()
            }
            if (state.manualOpen) {
                ManualPairSection(state.manual, actions)
            } else {
                TextPillButton(
                    if (state.link == null) "Enter the code by hand" else "Enter the code by hand instead",
                    actions::openManual,
                    modifier = Modifier.align(Alignment.CenterHorizontally),
                )
            }
            PairingAllowsCard()
            Text(
                "You can unpair from Settings on either device.",
                style = RostrumText.caption,
                color = colors.textSubtle,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(bottom = 20.dp),
            )
        }
    }
}

// --- previews ----------------------------------------------------------------

private val previewLink = PairingPreview(
    machine = "nymph-desk",
    hosts = listOf("192.168.1.24", "nymph-desk.local"),
    port = 8485,
    fingerprintShort = "4F2A · 91C0 · 7E3B",
    code = "WDJB-MJHT",
)

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun PairLinkPreview() {
    RostrumTheme {
        PairScreen(PairUiState(LinkState.Preview(previewLink, ActionState.Idle), manualOpen = false), NoPairActions, onBack = {})
    }
}

@Preview(widthDp = 412, heightDp = 1200)
@Composable
private fun PairManualPreview() {
    val probe = DesktopProbe("nymph-desk", 1, true, "192.168.1.24", 8485, "sha256:4F2A91C07E3B", "4F2A · 91C0 · 7E3B")
    RostrumTheme {
        PairScreen(
            PairUiState(
                link = null,
                manualOpen = true,
                manual = ManualForm(
                    host = "192.168.1.24",
                    code = "WDJB-MJHT",
                    step = ManualStep.PairFailed(probe, BackendError.RemoteApi(io.github.rhizonymph.rostrum.data.RemoteErrorCode.PairingCodeExpired, "expired")),
                ),
            ),
            NoPairActions,
            onBack = {},
        )
    }
}
