package io.github.rhizonymph.rostrum.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.CoreLink
import io.github.rhizonymph.rostrum.data.RustCore
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Root of the UI. Placeholder until the real screens and navigation land. */
@Composable
fun RostrumApp() {
    val link by produceState<CoreLink?>(initialValue = null) {
        value = withContext(Dispatchers.Default) { RustCore.probe() }
    }
    ShellScreen(link)
}

/** The wordmark and the Rust core's answer, which proves the `.so` is linked. */
@Composable
fun ShellScreen(link: CoreLink?, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.bg)
            .safeDrawingPadding()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = "rostrum",
            style = MaterialTheme.typography.displaySmall,
            fontWeight = FontWeight.SemiBold,
            color = colors.text,
        )
        val (line, color) = when (link) {
            null -> "loading core…" to colors.textMuted
            is CoreLink.Linked -> "rostrum_ffi ${link.ffiVersion}" to colors.accentText
            is CoreLink.Unavailable -> "core unavailable: ${link.reason}" to colors.dangerText
        }
        Text(text = line, style = RostrumTheme.mono.code, color = color)
    }
}

@Preview
@Composable
private fun ShellScreenPreview() {
    RostrumTheme { ShellScreen(CoreLink.Linked("0.1.0")) }
}
