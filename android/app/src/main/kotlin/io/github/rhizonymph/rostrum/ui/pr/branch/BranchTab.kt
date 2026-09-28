package io.github.rhizonymph.rostrum.ui.pr.branch

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.BaseDivergence
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.ui.components.CiGlyph
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.OutlinedPillButton
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.format.shortSha
import io.github.rhizonymph.rostrum.ui.pr.PrBusy
import io.github.rhizonymph.rostrum.ui.pr.common.CardBody
import io.github.rhizonymph.rostrum.ui.pr.common.CardLabel
import io.github.rhizonymph.rostrum.ui.pr.common.Fact
import io.github.rhizonymph.rostrum.ui.pr.common.baseUpdateExplanation
import io.github.rhizonymph.rostrum.ui.pr.common.branchFacts
import io.github.rhizonymph.rostrum.ui.pr.common.mergeStatusTitle
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The Branch tab: merge status, distance from base, and the desktop's worktree. */
@Composable
fun BranchTab(
    detail: PullDetail,
    busy: PrBusy?,
    local: BranchUiState,
    onUpdateBranch: (BranchUpdateMethod) -> Unit,
    localActions: LocalActions,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        MergeStatusCard(detail)
        BaseCard(detail.header, busy, onUpdateBranch)
        LocalCard(detail.header, local, localActions)
    }
}

@Composable
private fun MergeStatusCard(detail: PullDetail) {
    val colors = RostrumTheme.colors
    val verdict = detail.header.merge
    RostrumCard(Modifier.fillMaxWidth()) {
        CardBody {
            CardLabel("Merge status")
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                StatusDot(verdict.role.colors().solid, size = 8.dp)
                Text(mergeStatusTitle(detail.header), style = RostrumText.rowTitle.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
            }
            Text(verdict.sentence, style = RostrumText.meta.copy(lineHeight = RostrumText.body.lineHeight), color = colors.textMuted)
            Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                branchFacts(detail).forEach { FactTile(it, Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
private fun FactTile(fact: Fact, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val role = fact.role.colors()
    Column(
        modifier.background(colors.raised, RoundedCornerShape(12.dp)).padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            CiGlyph(fact.shape, role.solid, size = 14.dp, strokeWidth = 2.2f, contentDescription = null)
            Text(fact.label, style = RostrumText.caption, color = colors.textMuted)
        }
        Text(fact.value, style = RostrumText.meta.copy(fontWeight = FontWeight.Medium), color = role.text, maxLines = 2)
    }
}

@Composable
private fun BaseCard(header: PullHeader, busy: PrBusy?, onUpdateBranch: (BranchUpdateMethod) -> Unit) {
    val colors = RostrumTheme.colors
    var confirmRebase by rememberSaveable { mutableStateOf(false) }
    RostrumCard(Modifier.fillMaxWidth()) {
        CardBody {
            CardLabel("Base")
            val divergence = header.divergence
            when {
                divergence == null -> Text(
                    "The distance from ${header.baseRef} isn't known yet.",
                    style = RostrumText.meta,
                    color = colors.textMuted,
                )
                divergence.behind == 0 -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Up to date with ${header.baseRef}", style = RostrumText.rowTitle, color = colors.successText, modifier = Modifier.weight(1f))
                    Text("↑${divergence.ahead} ahead", style = RostrumText.mono13, color = colors.textMuted)
                }
                else -> BehindBase(header, divergence, busy, onUpdateBranch, onRequestRebase = { confirmRebase = true })
            }
        }
    }
    if (confirmRebase) {
        ConfirmDialog(
            title = "Rebase ${header.headRef}?",
            body = "GitHub replays the branch's commits onto ${header.baseRef}, which rewrites them. " +
                "Anyone with the branch checked out will need to reset to the new commits.",
            confirmLabel = "Rebase",
            onConfirm = {
                confirmRebase = false
                onUpdateBranch(BranchUpdateMethod.Rebase)
            },
            onDismiss = { confirmRebase = false },
        )
    }
}

@Composable
private fun BehindBase(
    header: PullHeader,
    divergence: BaseDivergence,
    busy: PrBusy?,
    onUpdateBranch: (BranchUpdateMethod) -> Unit,
    onRequestRebase: () -> Unit,
) {
    val colors = RostrumTheme.colors
    Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("↓${divergence.behind}", style = RostrumText.monoBig28, color = colors.warningText)
        Text(
            buildAnnotatedString {
                append("behind ")
                withStyle(SpanStyle(fontFamily = RostrumFonts.Mono, fontSize = RostrumText.label.fontSize)) { append(divergence.baseRef) }
            },
            style = RostrumText.rowTitle.copy(fontWeight = FontWeight.Normal),
            color = colors.text,
            modifier = Modifier.weight(1f).padding(bottom = 3.dp),
        )
        Text("↑${divergence.ahead} ahead", style = RostrumText.mono13, color = colors.textMuted, modifier = Modifier.padding(bottom = 4.dp))
    }
    Text(baseUpdateExplanation(divergence, header.headRef), style = RostrumText.meta, color = colors.textMuted)
    val open = header.state == PullState.Open
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedPillButton(
            "Update: Merge",
            onClick = { onUpdateBranch(BranchUpdateMethod.Merge) },
            enabled = open && busy == null,
            busy = busy == PrBusy.UpdateMerge,
            modifier = Modifier.weight(1f),
        )
        OutlinedPillButton(
            "Update: Rebase",
            onClick = onRequestRebase,
            enabled = open && busy == null,
            busy = busy == PrBusy.UpdateRebase,
            modifier = Modifier.weight(1f),
        )
    }
    Text("guarded by head ${shortSha(header.headSha)}", style = RostrumText.mono12, color = colors.textSubtle)
}
