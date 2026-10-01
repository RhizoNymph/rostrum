package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.IconTile
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.TonalButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** A desktop to pair with: tile, name, and the lines to compare. */
@Composable
internal fun DesktopIdentityCard(name: String, lines: List<String>, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        Row(
            Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            IconTile(size = 36.dp) {
                Icon(RostrumIcons.Desktop, contentDescription = null, tint = colors.onTonal, modifier = Modifier.size(20.dp))
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    name,
                    style = RostrumText.rowTitle.copy(fontWeight = FontWeight.SemiBold),
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                lines.forEach { Text(it, style = RostrumText.mono12, color = colors.textMuted) }
            }
        }
    }
}

/** The link's preview and the button that pairs with it. */
@Composable
internal fun LinkPreviewSection(preview: PairingPreview, pairing: ActionState, onPair: () -> Unit) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Check that the fingerprint matches the one on your desktop's pairing page, then pair.",
            style = RostrumText.body.copy(lineHeight = RostrumText.body.lineHeight),
            color = colors.textSecondary,
        )
        DesktopIdentityCard(
            name = preview.machine,
            lines = listOf(addressesLabel(preview.hosts, preview.port), "fingerprint ${preview.fingerprintShort}"),
        )
        (pairing as? ActionState.Failed)?.let { FieldError(it.error.describe()) }
        PrimaryButton(
            "Pair ${preview.machine}",
            onPair,
            modifier = Modifier.fillMaxWidth(),
            busy = pairing == ActionState.Running,
            height = 48.dp,
        )
    }
}

/** How to get a pairing link onto this phone. */
@Composable
internal fun PairInstructions() {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            pairingPageSentence("Rostrum on your desktop serves a pairing page at ", "."),
            style = RostrumText.body,
            color = colors.textSecondary,
        )
        Text(
            "Open it in this phone's browser and tap Open in Rostrum, or open it on the desktop and scan its QR code " +
                "with this phone's camera app. Rostrum opens here with the desktop ready to pair.",
            style = RostrumText.body,
            color = colors.textSecondary,
        )
    }
}

/** Address, port and code by hand; then the probed fingerprint to compare, then pair. */
@Composable
internal fun ManualPairSection(form: ManualForm, actions: PairActions) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        SectionHeader("Enter the code by hand")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                FieldLabel("Desktop address")
                RostrumTextField(
                    value = form.host,
                    onValueChange = actions::onHostChange,
                    placeholder = "192.168.1.24",
                    accessibilityLabel = "Desktop address",
                    mono = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false, imeAction = ImeAction.Next),
                )
            }
            Column(Modifier.width(104.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                FieldLabel("Port")
                RostrumTextField(
                    value = form.port,
                    onValueChange = actions::onPortChange,
                    accessibilityLabel = "Port",
                    mono = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Next),
                )
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            FieldLabel("Pairing code")
            RostrumTextField(
                value = form.code,
                onValueChange = actions::onCodeChange,
                placeholder = "XXXX-XXXX",
                accessibilityLabel = "Pairing code",
                mono = true,
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.Characters,
                    keyboardType = KeyboardType.Ascii,
                    autoCorrectEnabled = false,
                    imeAction = ImeAction.Done,
                ),
                keyboardActions = KeyboardActions(onDone = { if (form.probe == null) actions.probe() else actions.pairManual() }),
            )
        }
        when (val step = form.step) {
            ManualStep.Editing, ManualStep.Probing, is ManualStep.ProbeFailed -> {
                if (step is ManualStep.ProbeFailed) FieldError(step.error.describe())
                TonalButton(
                    "Check desktop",
                    actions::probe,
                    modifier = Modifier.fillMaxWidth(),
                    enabled = form.canProbe,
                    busy = step == ManualStep.Probing,
                    height = 48.dp,
                )
            }
            is ManualStep.Probed, is ManualStep.Pairing, is ManualStep.PairFailed -> {
                val probe = requireNotNull(form.probe)
                ProbeResult(probe)
                if (step is ManualStep.PairFailed) FieldError(step.error.describe())
                PrimaryButton(
                    "Pair ${probe.machine}",
                    actions::pairManual,
                    modifier = Modifier.fillMaxWidth(),
                    enabled = form.canPair,
                    busy = step is ManualStep.Pairing,
                    height = 48.dp,
                )
                if (!form.canPair && probe.compatible) {
                    Text("Enter all eight characters of the code.", style = RostrumText.caption, color = colors.textSubtle)
                }
            }
        }
    }
}

@Composable
private fun ProbeResult(probe: DesktopProbe) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            "Compare this fingerprint with the one on the desktop's pairing page. Pair only if they match.",
            style = RostrumText.meta,
            color = colors.textSecondary,
        )
        DesktopIdentityCard(
            name = probe.machine,
            lines = listOf("${probe.host} :${probe.port}", "fingerprint ${probe.fingerprintShort}"),
        )
        if (!probe.compatible) {
            NoticeBanner("This desktop speaks API version ${probe.apiVersion}, which this app doesn't. Update the older of the two.")
        }
    }
}

/** What the desktop can do for this phone once paired. */
@Composable
internal fun PairingAllowsCard() {
    val colors = RostrumTheme.colors
    RostrumCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("What pairing allows", style = RostrumText.meta.copy(fontWeight = FontWeight.Medium), color = colors.textMuted)
            CheckLine("See which worktree each pull request is checked out in")
            CheckLine("Pull, merge and rebase local branches")
            CheckLine("Hand conflicts to your terminal and abort them")
            CheckLine("Never pushes, and never reads files outside your configured clones")
        }
    }
}
