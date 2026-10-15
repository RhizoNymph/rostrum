package io.github.rhizonymph.rostrum.ui.ci

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.CiAnnotation
import io.github.rhizonymph.rostrum.data.model.CiAnnotationLevel
import io.github.rhizonymph.rostrum.data.model.CiCheckOutput
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.CiGlyph
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.OutlinedPillButton
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.components.markdown.MarkdownBlocks
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * A cell's bottom sheet: the check's header and Retry menu, then its log
 * (Actions), output and annotations (another app), or only its link (a
 * legacy status).
 */
@Composable
fun CiCellSheet(sheet: CiSheet, actions: CiGridActions) {
    RostrumBottomSheet(onDismiss = actions::closeSheet) {
        val tall = sheet.detail is CiDetail.Log
        Column(
            Modifier
                .then(if (tall) Modifier.fillMaxHeight(0.92f) else Modifier)
                .padding(horizontal = 16.dp)
                .padding(bottom = 12.dp)
                .navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            SheetHeader(sheet, actions)
            RetryPanel(sheet, actions)
            when (val detail = sheet.detail) {
                is CiDetail.Log -> when (val log = detail.log) {
                    UiState.Loading -> LoadingView(label = "Reading the log")
                    is UiState.Error -> ErrorView(log.error, title = "Couldn't read the log", onRetry = actions::retryDetail)
                    is UiState.Loaded -> CiLogView(detail, log.data, actions, Modifier.weight(1f))
                }
                is CiDetail.Output -> when (val output = detail.output) {
                    UiState.Loading -> LoadingView(label = "Reading the check's output")
                    is UiState.Error -> ErrorView(output.error, title = "Couldn't read the output", onRetry = actions::retryDetail)
                    is UiState.Loaded -> OutputView(output.data)
                }
                CiDetail.Status -> Text(
                    "Reported by ${sheet.target.cell.producer}. Its details are on the provider's site.",
                    style = RostrumText.body,
                    color = RostrumTheme.colors.textSecondary,
                )
            }
        }
    }
    (sheet.retry as? RetryState.Confirm)?.let { confirm ->
        ConfirmDialog(
            title = confirm.option.label,
            body = confirm.option.confirmPrompt,
            confirmLabel = "Re-run",
            onConfirm = actions::confirmRerun,
            onDismiss = actions::dismissRetry,
        )
    }
}

@Composable
private fun SheetHeader(sheet: CiSheet, actions: CiGridActions) {
    val colors = RostrumTheme.colors
    val cell = sheet.target.cell
    val uriHandler = LocalUriHandler.current
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 4.dp)) {
        CiGlyph(cell.status.shape(), cell.role.colors().solid, size = 20.dp, contentDescription = null)
        Text(
            sheet.target.column.label,
            style = RostrumText.sheetTitle,
            color = colors.text,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        StatusChip(cell.statusLabel, cell.role)
    }
    Text(
        "#${sheet.target.number} ${sheet.target.title}",
        style = RostrumText.meta,
        color = colors.textSecondary,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
    Text(
        listOfNotNull(cell.durationLabel ?: cell.timingLabel, cell.producer).joinToString(" · "),
        style = RostrumText.caption,
        color = colors.textMuted,
    )
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        val retryOpen = sheet.retry !is RetryState.Closed && sheet.retry !is RetryState.Confirm
        OutlinedPillButton("Retry…", actions::openRetry, enabled = !retryOpen)
        cell.detailsUrl?.let { url ->
            if (sheet.detail == CiDetail.Status) {
                PrimaryButton("Open in browser", { uriHandler.openUri(url) })
            } else {
                TextPillButton("Open in browser", { uriHandler.openUri(url) })
            }
        }
    }
}

/** The Retry menu: the core's options (or why there are none), progress, or a typed refusal. */
@Composable
private fun RetryPanel(sheet: CiSheet, actions: CiGridActions) {
    val colors = RostrumTheme.colors
    val retry = sheet.retry
    if (retry is RetryState.Closed || retry is RetryState.Confirm) return
    Column(
        Modifier.fillMaxWidth().background(colors.surface, RoundedCornerShape(12.dp)).padding(vertical = 4.dp),
    ) {
        when (retry) {
            RetryState.Closed, is RetryState.Confirm -> Unit
            RetryState.Loading -> PanelText("Checking what can be re-run…")
            is RetryState.Running -> PanelText("Asking GitHub to ${retry.option.label.replaceFirstChar { it.lowercase() }}…")
            is RetryState.Failed -> {
                FieldError(retry.error.describe(), Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
                PanelClose(actions)
            }
            is RetryState.Menu -> when (val choice = retry.choice) {
                is CiRerunChoice.Unavailable -> {
                    PanelText(choice.message)
                    PanelClose(actions)
                }
                is CiRerunChoice.Available -> {
                    choice.options.forEach { option ->
                        Text(
                            option.label,
                            style = RostrumText.rowTitle,
                            color = colors.text,
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 48.dp)
                                .clickable(role = Role.Button) { actions.chooseRerun(option) }
                                .padding(horizontal = 14.dp, vertical = 13.dp),
                        )
                    }
                    PanelClose(actions, label = "Cancel")
                }
            }
        }
    }
}

@Composable
private fun PanelText(text: String) {
    Text(text, style = RostrumText.meta, color = RostrumTheme.colors.textSecondary, modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp))
}

@Composable
private fun PanelClose(actions: CiGridActions, label: String = "Close") {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
        TextPillButton(label, actions::dismissRetry)
    }
}

@Composable
private fun OutputView(output: CiCheckOutput) {
    val colors = RostrumTheme.colors
    Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        output.title?.let { Text(it, style = RostrumText.cardTitle, color = colors.text) }
        if (output.summary.isNotEmpty()) MarkdownBlocks(output.summary)
        if (output.text.isNotEmpty()) MarkdownBlocks(output.text)
        if (output.annotations.isNotEmpty()) {
            Text("Annotations · ${output.annotations.size}", style = RostrumText.section, color = colors.textMuted)
            output.annotations.forEach { AnnotationRow(it) }
        }
    }
}

@Composable
private fun AnnotationRow(annotation: CiAnnotation) {
    val colors = RostrumTheme.colors
    val role = when (annotation.level) {
        CiAnnotationLevel.Failure -> ColorRole.Danger
        CiAnnotationLevel.Warning -> ColorRole.Warning
        CiAnnotationLevel.Notice -> ColorRole.Accent
    }
    Column(
        Modifier.fillMaxWidth().background(colors.surface, RoundedCornerShape(10.dp)).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            StatusChip(annotation.level.name.lowercase(), role)
            Text(annotation.location, style = RostrumText.mono12, color = colors.textSecondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        annotation.title?.let { Text(it, style = RostrumText.label, color = colors.text) }
        Text(annotation.message, style = RostrumText.meta, color = colors.textSecondary)
    }
}
