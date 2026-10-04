package io.github.rhizonymph.rostrum.ui.stacks

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.StackJobState
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanCheck
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.CheckboxVisual
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.CopyCommandRow
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The open stack question, picker, confirmation or job, if any. */
@Composable
fun StackActionsHost(flow: StackFlow?, actions: StackActions, onPairDesktop: () -> Unit) {
    when (flow) {
        null -> Unit
        is StackFlow.NeedsDesktop -> ConfirmDialog(
            title = "${flow.action} needs your desktop",
            body = "Stack actions run on the paired desktop, which rebases and pushes from its clone. Pair one to use them.",
            confirmLabel = "Pair a desktop",
            onConfirm = {
                actions.dismiss()
                onPairDesktop()
            },
            onDismiss = actions::dismiss,
        )
        is StackFlow.ConfirmUnstack -> ConfirmDialog(
            title = "Unstack ${flow.title}?",
            body = "GitHub stops treating these pull requests as a stack. Their branches and bases stay as they are." +
                ((flow.run as? ActionState.Failed)?.let { "\n\n${it.error.describe()}" } ?: ""),
            confirmLabel = if (flow.run.running) "Unstacking…" else "Unstack",
            onConfirm = actions::confirm,
            onDismiss = actions::dismiss,
            destructive = true,
        )
        is StackFlow.ConfirmMerge -> Sheet(actions) { MergeContent(flow, actions) }
        is StackFlow.ConfirmMake -> Sheet(actions) { MakeContent(flow, actions) }
        is StackFlow.PickExtend -> Sheet(actions) { ExtendContent(flow, actions) }
        is StackFlow.OrderArrange -> Sheet(actions) { ArrangeContent(flow, actions) }
        is StackFlow.ConfirmRewrite -> Sheet(actions) { RewriteContent(flow, actions) }
        is StackFlow.Job -> Sheet(actions) { JobContent(flow, actions) }
    }
}

@Composable
private fun Sheet(actions: StackActions, content: @Composable ColumnScope.() -> Unit) {
    RostrumBottomSheet(onDismiss = actions::dismiss) {
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding().imePadding().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            content = content,
        )
    }
}

@Composable
private fun Title(text: String) {
    Text(text, style = RostrumText.sheetTitle, color = RostrumTheme.colors.text, modifier = Modifier.semantics { heading() })
}

@Composable
private fun Body(text: String) {
    Text(text, style = RostrumText.body, color = RostrumTheme.colors.textSecondary)
}

@Composable
private fun Buttons(run: ActionState, confirmLabel: String, enabled: Boolean, onConfirm: () -> Unit, onCancel: () -> Unit) {
    (run as? ActionState.Failed)?.let { FieldError(it.error.describe()) }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
        TextPillButton("Cancel", onCancel, enabled = !run.running)
        PrimaryButton(confirmLabel, onConfirm, enabled = enabled && !run.running, busy = run.running)
    }
}

@Composable
private fun MemberLine(pr: PrSummary, index: Int? = null) {
    val colors = RostrumTheme.colors
    Row(Modifier.fillMaxWidth().heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        index?.let { Text("${it + 1}.", style = RostrumText.mono12, color = colors.textMuted) }
        Text("#${pr.number}", style = RostrumText.mono12, color = colors.textMuted)
        Text(pr.title, style = RostrumText.label, color = colors.text, maxLines = 1, modifier = Modifier.weight(1f))
        pr.mergeChip?.let { StatusChip(it) }
    }
}

@Composable
private fun MergeContent(flow: StackFlow.ConfirmMerge, actions: StackActions) {
    Title("Merge ${flow.title}?")
    flow.members.forEach { MemberLine(it) }
    SegmentedToggle(
        options = StackMergeMethod.entries.toList(),
        selected = flow.method,
        onSelect = actions::setMergeMethod,
        label = { it.name },
        modifier = Modifier.fillMaxWidth(),
    )
    Body(MERGE_NOTE)
    Buttons(flow.run, "Merge stack", enabled = true, onConfirm = actions::confirm, onCancel = actions::dismiss)
}

@Composable
private fun MakeContent(flow: StackFlow.ConfirmMake, actions: StackActions) {
    Title("Make a stack of ${flow.members.size} pull requests?")
    Body("They already chain on ${flow.trunk}, bottom first. GitHub records them as a stack; nothing is rewritten or pushed.")
    flow.members.forEachIndexed { index, pr -> MemberLine(pr, index) }
    Buttons(flow.run, "Make stack", enabled = true, onConfirm = actions::confirm, onCancel = actions::dismiss)
}

@Composable
private fun ExtendContent(flow: StackFlow.PickExtend, actions: StackActions) {
    val colors = RostrumTheme.colors
    Title("Add to ${flow.title}")
    Body("Pick pull requests to add on top, in order. The desktop says which branches it would rewrite before anything is pushed.")
    when (val candidates = flow.candidates) {
        UiState.Loading -> LoadingView(label = "Finding pull requests…")
        is UiState.Error -> ErrorView(candidates.error, title = "Couldn't list pull requests")
        is UiState.Loaded -> if (candidates.data.isEmpty()) {
            Body("No other open pull requests in this repository.")
        } else {
            candidates.data.forEach { candidate ->
                val order = flow.chosen.indexOf(candidate.number)
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 52.dp)
                        .toggleable(order >= 0, enabled = candidate.pickable, role = Role.Checkbox, onValueChange = { actions.toggleCandidate(candidate.number) }),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    CheckboxVisual(order >= 0)
                    Column(Modifier.weight(1f)) {
                        Text("#${candidate.number} ${candidate.title}", style = RostrumText.label, color = if (candidate.pickable) colors.text else colors.textSubtle, maxLines = 1)
                        Text(candidateNote(candidate), style = RostrumText.caption, color = colors.textMuted)
                    }
                    if (order >= 0) Text("${order + 1}", style = RostrumText.mono12, color = colors.accentText)
                }
            }
        }
    }
    Buttons(flow.run, "Next", enabled = flow.chosen.isNotEmpty(), onConfirm = actions::planExtend, onCancel = actions::dismiss)
}

@Composable
private fun ArrangeContent(flow: StackFlow.OrderArrange, actions: StackActions) {
    val colors = RostrumTheme.colors
    Title("Arrange ${flow.members.size} pull requests")
    Body("Bottom first. Each one will be based on the one before it, the first on the trunk.")
    flow.members.forEachIndexed { index, pr ->
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) { MemberLine(pr, index) }
            RostrumIconButton(RostrumIcons.ChevronUp, "Move #${pr.number} down the stack", { actions.moveUp(index) }, enabled = index > 0, iconSize = 18.dp)
            RostrumIconButton(RostrumIcons.ChevronDown, "Move #${pr.number} up the stack", { actions.moveDown(index) }, enabled = index < flow.members.lastIndex, iconSize = 18.dp)
        }
    }
    RostrumTextField(value = flow.trunk, onValueChange = actions::setTrunk, placeholder = "Trunk", accessibilityLabel = "Trunk branch", mono = true)
    when (val check = flow.check) {
        null -> Unit
        is StackPlanCheck.Valid -> Text(
            if (check.rewrites.isEmpty()) "They already chain; nothing needs rewriting." else "${check.rewrites.size} branch${if (check.rewrites.size == 1) "" else "es"} would be rewritten.",
            style = RostrumText.caption,
            color = colors.textMuted,
        )
        is StackPlanCheck.Invalid -> FieldError(check.reason)
    }
    Buttons(flow.run, "Next", enabled = flow.check !is StackPlanCheck.Invalid && flow.trunk.isNotBlank(), onConfirm = actions::planArrange, onCancel = actions::dismiss)
}

@Composable
private fun RewriteContent(flow: StackFlow.ConfirmRewrite, actions: StackActions) {
    val colors = RostrumTheme.colors
    val what = if (flow.request is StackPlanRequest.Arrange) "Arrange" else "Add"
    Title(if (flow.rewrites.isEmpty()) "$what without rewriting?" else "Rewrite ${flow.rewrites.size} branch${if (flow.rewrites.size == 1) "" else "es"}?")
    flow.note?.let { FieldError("The desktop's plan changed: $it") }
    if (flow.rewrites.isEmpty()) {
        Body("Nothing needs rebasing; the desktop links the pull requests as a stack.")
    } else {
        Body("The desktop rebases these branches and force-pushes them (with lease). Only these:")
        flow.rewrites.forEach { rewrite ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("#${rewrite.number}", style = RostrumText.mono12, color = colors.textMuted)
                Text(rewrite.branch, style = RostrumText.mono12, color = colors.text)
            }
        }
    }
    Buttons(flow.run, if (flow.rewrites.isEmpty()) what else "Rewrite and push", enabled = true, onConfirm = actions::confirm, onCancel = actions::dismiss)
}

@Composable
private fun JobContent(flow: StackFlow.Job, actions: StackActions) {
    val colors = RostrumTheme.colors
    val job = flow.job
    Title(jobTitle(job.kind))
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (!job.finished) CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
        Text(jobOutcome(job), style = RostrumText.body, color = colors.text)
    }
    when (val state = job.state) {
        is StackJobState.HandedOff -> {
            Body("Finish the rebase in ${state.worktree}, then run the action again.")
            CopyCommandRow(attachCommand(state.session))
        }
        is StackJobState.Failed -> {
            if (state.pushed.isNotEmpty()) Body("Already force-pushed: ${numbers(state.pushed)}.")
            Body(state.detail)
        }
        is StackJobState.Conflicted -> Body(state.detail)
        is StackJobState.Done, is StackJobState.Running -> Unit
    }
    if (!job.finished) Body("It keeps running on the desktop if you close this.")
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
        PrimaryButton(if (job.finished) "Done" else "Close", actions::dismiss)
    }
}
