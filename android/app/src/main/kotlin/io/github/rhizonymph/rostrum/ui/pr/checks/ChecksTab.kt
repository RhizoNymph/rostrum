package io.github.rhizonymph.rostrum.ui.pr.checks

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.CheckRunView
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.CiGlyph
import io.github.rhizonymph.rostrum.ui.components.CiShape
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.format.relativeAgo
import io.github.rhizonymph.rostrum.ui.format.shortSha
import io.github.rhizonymph.rostrum.ui.pr.common.checkRunShape
import io.github.rhizonymph.rostrum.ui.pr.common.checksSummary
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** The Checks tab: a summary of the head commit's CI, then every run. */
@Composable
fun ChecksTab(
    detail: PullDetail,
    loadedAt: Instant?,
    now: Instant,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val summary = checksSummary(detail.checks)
    LazyColumn(
        modifier = modifier,
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "summary") {
            RostrumCard(Modifier.fillMaxWidth()) {
                Row(
                    Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    val role = summary.role.colors()
                    Box(Modifier.size(40.dp).background(role.tint, CircleShape), contentAlignment = Alignment.Center) {
                        CiGlyph(summary.shape, role.solid, size = 22.dp, contentDescription = null)
                    }
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(summary.title, style = RostrumText.rowTitle.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
                        Text(summary.subtitle, style = RostrumText.meta, color = colors.textMuted)
                    }
                    Text("head ${shortSha(detail.header.headSha)}", style = RostrumText.mono12, color = colors.textSubtle)
                }
            }
        }
        if (detail.checks.isNotEmpty()) {
            item(key = "runs-header") { SectionHeader("Runs", Modifier.padding(top = 4.dp)) }
            item(key = "runs") {
                RostrumCard(Modifier.fillMaxWidth()) {
                    detail.checks.forEachIndexed { index, run ->
                        if (index > 0) CardDivider()
                        CheckRunRow(run)
                    }
                }
            }
        }
        if (loadedAt != null) {
            item(key = "footnote") {
                Text(
                    "Refreshed with the feed · ${relativeAgo(loadedAt, now)}",
                    style = RostrumText.caption,
                    color = colors.textSubtle,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
                )
            }
        }
    }
}

@Composable
private fun CheckRunRow(run: CheckRunView) {
    val colors = RostrumTheme.colors
    val uriHandler = LocalUriHandler.current
    val shape = checkRunShape(run)
    Row(
        Modifier.fillMaxWidth().height(60.dp).padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        CiGlyph(shape, run.role.colors().solid)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                run.name,
                style = RostrumText.label,
                color = if (shape == CiShape.Skipped || shape == CiShape.None) colors.textSecondary else colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(run.statusText, style = RostrumText.mono12, color = colors.textMuted)
        }
        val url = run.url
        if (url != null) {
            RostrumIconButton(
                RostrumIcons.OpenInBrowser,
                "Open ${run.name} in browser",
                onClick = { uriHandler.openUri(url) },
                tint = colors.textMuted,
                iconSize = 18.dp,
            )
        } else {
            Spacer(Modifier.size(48.dp))
        }
    }
}
