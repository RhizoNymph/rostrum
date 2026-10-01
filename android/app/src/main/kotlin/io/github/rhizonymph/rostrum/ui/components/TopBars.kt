package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** A top-level screen's 64dp header: `Settings`, `Desktop`. */
@Composable
fun ScreenHeader(
    title: String,
    modifier: Modifier = Modifier,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Row(
        modifier = modifier.fillMaxWidth().height(64.dp).padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            style = RostrumText.screenTitle,
            color = RostrumTheme.colors.text,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        actions()
    }
}

/**
 * A pushed screen's 64dp header: back button, then [content] (title lines),
 * then [actions].
 */
@Composable
fun BackTopBar(
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    backDescription: String = "Back",
    actions: @Composable RowScope.() -> Unit = {},
    content: @Composable () -> Unit,
) {
    Row(
        modifier = modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RostrumIconButton(RostrumIcons.ArrowBack, backDescription, onBack, tint = RostrumTheme.colors.textSecondary)
        Box(Modifier.weight(1f).padding(start = 4.dp)) { content() }
        actions()
    }
}

/** Two-line title for [BackTopBar]: a small line above a strong one. */
@Composable
fun TitleStack(overline: String, title: @Composable () -> Unit) {
    Column {
        Text(overline, style = RostrumText.caption.copy(lineHeight = RostrumText.chip.lineHeight), color = RostrumTheme.colors.textMuted, maxLines = 1)
        title()
    }
}
