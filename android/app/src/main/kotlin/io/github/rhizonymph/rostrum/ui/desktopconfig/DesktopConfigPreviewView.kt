package io.github.rhizonymph.rostrum.ui.desktopconfig

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.format.MINUS
import io.github.rhizonymph.rostrum.ui.format.splitRepo
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The desktop's repositories (ones this phone lacks marked "+"), the ones
 * copying would drop from this phone, and a line summing up the feed
 * preferences that come with them.
 */
@Composable
fun DesktopConfigPreviewView(
    preview: DesktopConfigPreview,
    modifier: Modifier = Modifier,
    changes: List<String> = emptyList(),
) {
    val colors = RostrumTheme.colors
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (changes.isNotEmpty()) {
            Column(Modifier.padding(horizontal = 4.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                changes.forEach { line ->
                    Text("• $line", style = RostrumText.meta, color = colors.textSecondary)
                }
            }
        }
        SectionHeader("Repositories on ${preview.machine}", trailing = preview.repos.size.toString())
        RostrumCard(Modifier.fillMaxWidth()) {
            val rows = DesktopConfigText.repoRows(preview)
            if (rows.isEmpty()) {
                Text(
                    "None",
                    style = RostrumText.meta,
                    color = colors.textMuted,
                    modifier = Modifier.padding(14.dp),
                )
            }
            rows.forEachIndexed { index, row ->
                if (index > 0) CardDivider()
                RepoLine(
                    repo = row.repo,
                    marker = if (row.added) "+" else null,
                    markerColor = colors.successText,
                    description = if (row.added) "${row.repo}, added to this phone" else row.repo,
                )
            }
        }
        if (preview.removed.isNotEmpty()) {
            SectionHeader("Removed from this phone", trailing = preview.removed.size.toString(), trailingColor = colors.dangerText)
            RostrumCard(Modifier.fillMaxWidth()) {
                preview.removed.forEachIndexed { index, repo ->
                    if (index > 0) CardDivider()
                    RepoLine(
                        repo = repo,
                        marker = MINUS,
                        markerColor = colors.dangerText,
                        description = "$repo, removed from this phone",
                        struck = true,
                    )
                }
            }
        }
        Text(
            DesktopConfigText.preferencesSummary(preview),
            style = RostrumText.caption,
            color = colors.textMuted,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@Composable
private fun RepoLine(
    repo: String,
    marker: String?,
    markerColor: androidx.compose.ui.graphics.Color,
    description: String,
    struck: Boolean = false,
) {
    val colors = RostrumTheme.colors
    val (owner, name) = splitRepo(repo)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .padding(horizontal = 14.dp)
            .clearAndSetSemantics { contentDescription = description },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            marker ?: "",
            style = RostrumText.monoStrong13,
            color = markerColor,
            modifier = Modifier.width(18.dp),
        )
        val decoration = if (struck) TextDecoration.LineThrough else null
        Text(
            owner,
            style = RostrumText.label.copy(textDecoration = decoration),
            color = colors.textMuted,
            maxLines = 1,
        )
        Text(
            name,
            style = RostrumText.label.copy(textDecoration = decoration),
            color = if (struck) colors.textMuted else colors.text,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Preview
@Composable
private fun DesktopConfigPreviewViewPreview() {
    RostrumTheme {
        DesktopConfigPreviewView(
            DesktopConfigPreview(
                machine = "framework",
                repos = listOf("RhizoNymph/rostrum", "zed-industries/zed", "serde-rs/serde"),
                added = listOf("serde-rs/serde"),
                removed = listOf("rust-lang/rust"),
                prsPerRepo = 25,
                hideDrafts = true,
                hideEmptyRepos = true,
                authors = emptyList(),
                includeInvolved = false,
                autostash = true,
                changesAnything = true,
            ),
        )
    }
}
