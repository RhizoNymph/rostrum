package io.github.rhizonymph.rostrum.ui.items

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.StackSummary
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** Stack actions (merge, unstack, make stack) arrive with phase 2b, through the desktop. */
object StackFlags {
    /** Off until stack actions exist; the header's overflow menu stays hidden. */
    const val ACTIONS_ENABLED = false
}

/**
 * A stack's header above its members: chain icon, `Stack 7 · 2 PRs`, the
 * trunk (and members not open), and the merge rollup chip.
 */
@Composable
fun StackHeaderRow(stack: StackSummary, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier.fillMaxWidth().padding(start = 14.dp, end = 4.dp, top = 10.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Icon(RostrumIcons.Link, contentDescription = null, tint = colors.accentText, modifier = Modifier.size(16.dp))
        Column(Modifier.weight(1f)) {
            Text(
                stack.title,
                style = RostrumText.label.copy(fontWeight = FontWeight.SemiBold),
                color = colors.text,
                modifier = Modifier.semantics { heading() },
            )
            Text(stackSubline(stack), style = RostrumText.mono12, color = colors.textMuted)
        }
        stack.rollup?.let { StatusChip(it.label, it.role) }
        if (StackFlags.ACTIONS_ENABLED) StackMenu(stack.title)
    }
}

/** The stack's actions; empty until phase 2b. */
@Composable
private fun StackMenu(title: String) {
    var open by remember { mutableStateOf(false) }
    Box {
        RostrumIconButton(RostrumIcons.MoreVert, "Actions for $title", onClick = { open = true }, iconSize = 20.dp)
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = RostrumTheme.colors.raised) {}
    }
}
