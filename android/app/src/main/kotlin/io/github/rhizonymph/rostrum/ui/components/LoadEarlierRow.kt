package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** `Load earlier (3 more)`, or `Load earlier` when GitHub didn't say how many. */
fun loadEarlierText(count: Int?): String = if (count == null || count <= 0) "Load earlier" else "Load earlier ($count more)"

/** The row above a paged conversation's oldest loaded entry. */
@Composable
fun LoadEarlierRow(count: Int?, loading: Boolean, onLoad: () -> Unit, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().heightIn(min = 48.dp), contentAlignment = Alignment.Center) {
        if (loading) {
            CircularProgressIndicator(color = RostrumTheme.colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
        } else {
            TextPillButton(loadEarlierText(count), onLoad)
        }
    }
}
