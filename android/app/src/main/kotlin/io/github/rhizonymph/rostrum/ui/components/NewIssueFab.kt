package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** "+ New issue", on the feed's Issues tab and the repository screen's. */
@Composable
fun NewIssueFab(onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    ExtendedFloatingActionButton(
        onClick = onClick,
        modifier = modifier,
        shape = RoundedCornerShape(16.dp),
        containerColor = colors.accent,
        contentColor = colors.onAccent,
        icon = { Icon(RostrumIcons.Plus, contentDescription = null, modifier = Modifier.size(20.dp)) },
        text = { Text("New issue", style = RostrumText.button) },
    )
}
