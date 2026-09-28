package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** Centred spinner with an optional line under it. */
@Composable
fun LoadingView(modifier: Modifier = Modifier, label: String? = null) {
    val colors = RostrumTheme.colors
    Column(
        modifier = modifier.fillMaxWidth().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        CircularProgressIndicator(color = colors.accent, strokeWidth = 2.5.dp, modifier = Modifier.size(28.dp))
        if (label != null) Text(label, style = RostrumText.meta, color = colors.textMuted)
    }
}

/** A failed load: the error in the core's words, and Retry when it can help. */
@Composable
fun ErrorView(
    error: BackendError,
    modifier: Modifier = Modifier,
    title: String = "Couldn't load this",
    onRetry: (() -> Unit)? = null,
) {
    ErrorView(error.describe(), modifier, title, onRetry)
}

@Composable
fun ErrorView(
    message: String,
    modifier: Modifier = Modifier,
    title: String = "Couldn't load this",
    onRetry: (() -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        Column(
            Modifier
                .padding(16.dp)
                .semantics { liveRegion = LiveRegionMode.Polite },
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.dangerText, modifier = Modifier.size(18.dp))
                Text(title, style = RostrumText.rowTitle, color = colors.text)
            }
            Text(message, style = RostrumText.meta, color = colors.textMuted)
            if (onRetry != null) {
                TextPillButton("Retry", onRetry, modifier = Modifier.padding(start = 0.dp))
            }
        }
    }
}

/** Nothing to show, said plainly, with an optional action. */
@Composable
fun EmptyView(
    title: String,
    modifier: Modifier = Modifier,
    body: String? = null,
    action: (@Composable () -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    Column(
        modifier = modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(title, style = RostrumText.rowTitle, color = colors.textSecondary, textAlign = TextAlign.Center)
        if (body != null) Text(body, style = RostrumText.meta, color = colors.textMuted, textAlign = TextAlign.Center)
        if (action != null) Box(Modifier.padding(top = 4.dp)) { action() }
    }
}

/** An inline error line under a field: danger icon and text. */
@Composable
fun FieldError(text: String, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier
            .padding(horizontal = 4.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.dangerText, modifier = Modifier.size(14.dp))
        Text(text, style = RostrumText.caption, color = colors.dangerText)
    }
}
