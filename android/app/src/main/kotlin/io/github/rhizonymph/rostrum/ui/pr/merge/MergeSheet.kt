package io.github.rhizonymph.rostrum.ui.pr.merge

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.pr.common.mergeSheetFacts
import io.github.rhizonymph.rostrum.ui.pr.common.methodLabel
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The merge sheet's callbacks. */
data class MergeActions(
    val onMethod: (MergeMethod) -> Unit,
    val onTitle: (String) -> Unit,
    val onMessage: (String) -> Unit,
    val onConfirm: () -> Unit,
    val onDismiss: () -> Unit,
)

/** Method, the facts it rests on, the commit text, and the explicit confirmation. */
@Composable
fun MergeSheet(detail: PullDetail, form: MergeFormState, actions: MergeActions) {
    RostrumBottomSheet(onDismiss = actions.onDismiss) {
        MergeSheetContent(detail, form, actions)
    }
}

@Composable
fun MergeSheetContent(detail: PullDetail, form: MergeFormState, actions: MergeActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val header = detail.header
    Column(
        modifier = modifier
            .fillMaxWidth()
            .imePadding()
            .navigationBarsPadding()
            .verticalScroll(rememberScrollState())
            .padding(start = 20.dp, end = 20.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Column(Modifier.padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(
                buildAnnotatedString {
                    append("Merge ")
                    withStyle(SpanStyle(fontFamily = RostrumFonts.Mono, fontSize = 19.sp)) { append("#${header.number}") }
                    append(" into ${header.baseRef}")
                },
                style = RostrumText.sheetTitle,
                color = colors.text,
            )
            Text("${header.headRef} → ${header.baseRef}", style = RostrumText.mono12, color = colors.textMuted)
        }
        SegmentedToggle(
            options = MergeMethod.entries,
            selected = form.method,
            onSelect = actions.onMethod,
            label = ::methodLabel,
            modifier = Modifier.fillMaxWidth(),
        )
        FactsCard(detail)
        if (form.hasCommitText) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Commit title", style = RostrumText.section, color = colors.textMuted)
                RostrumTextField(
                    value = form.title,
                    onValueChange = actions.onTitle,
                    placeholder = "GitHub's default title",
                    accessibilityLabel = "Commit title",
                    fill = colors.bg,
                    enabled = !form.submitting,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Commit message", style = RostrumText.section, color = colors.textMuted)
                RostrumTextField(
                    value = form.message,
                    onValueChange = actions.onMessage,
                    placeholder = "Optional",
                    accessibilityLabel = "Commit message",
                    singleLine = false,
                    minHeight = 72.dp,
                    fill = colors.bg,
                    enabled = !form.submitting,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        } else {
            Text(
                "Rebase replays each commit onto ${header.baseRef} as it is; no merge commit is written.",
                style = RostrumText.meta,
                color = colors.textMuted,
            )
        }
        form.error?.let { FieldError(it.describe()) }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextPillButton("Cancel", actions.onDismiss, enabled = !form.submitting)
            ConfirmMergeButton(form.submitting, actions.onConfirm, Modifier.weight(1f))
        }
        Text(
            "Merging can't be undone from Rostrum.",
            style = RostrumText.caption,
            color = colors.textSubtle,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun FactsCard(detail: PullDetail) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(12.dp)
    Column(
        Modifier.fillMaxWidth().background(colors.surface, shape).border(1.dp, colors.border, shape).padding(horizontal = 12.dp),
    ) {
        mergeSheetFacts(detail).forEachIndexed { index, fact ->
            if (index > 0) CardDivider()
            Row(Modifier.fillMaxWidth().height(40.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(fact.label, style = RostrumText.label.copy(fontWeight = RostrumText.body.fontWeight), color = colors.textSecondary, modifier = Modifier.weight(1f))
                Text(fact.value, style = RostrumText.meta, color = fact.role.colors().text)
            }
        }
    }
}

@Composable
private fun ConfirmMergeButton(busy: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Button(
        onClick = onClick,
        enabled = !busy,
        modifier = modifier.height(48.dp),
        shape = CircleShape,
        colors = ButtonDefaults.buttonColors(
            containerColor = colors.merge,
            contentColor = colors.onMerge,
            disabledContainerColor = colors.merge,
            disabledContentColor = colors.onMerge,
        ),
        contentPadding = PaddingValues(horizontal = 20.dp),
    ) {
        if (busy) {
            CircularProgressIndicator(color = colors.onMerge, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
        } else {
            Icon(RostrumIcons.Merge, contentDescription = null, modifier = Modifier.size(18.dp))
            Text("Confirm merge", style = RostrumText.button.copy(fontSize = 15.sp), modifier = Modifier.padding(start = 8.dp))
        }
    }
}
