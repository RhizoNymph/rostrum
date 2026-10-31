package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The budget chip, tappable for its explanation. */
@Composable
fun BudgetChip(view: BudgetChipView, onClick: () -> Unit, modifier: Modifier = Modifier) {
    StatusChip(
        text = view.text,
        role = view.role,
        mono = true,
        modifier = modifier
            .clickable(onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = view.description
                role = Role.Button
            },
    )
}

/** What the budget chip opens: [budgetExplanation]'s paragraphs. */
@Composable
fun BudgetSheet(explanation: BudgetExplanation, onDone: () -> Unit) {
    RostrumBottomSheet(onDismiss = onDone) {
        BudgetSheetContent(explanation, onDone)
    }
}

@Composable
fun BudgetSheetContent(explanation: BudgetExplanation, onDone: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(
        modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .verticalScroll(rememberScrollState())
            .padding(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(explanation.title, style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.semantics { heading() })
        explanation.lines.forEach { line ->
            Text(line, style = RostrumText.body, color = colors.textMuted)
        }
        PrimaryButton("Done", onClick = onDone, modifier = Modifier.fillMaxWidth().padding(top = 4.dp))
    }
}
