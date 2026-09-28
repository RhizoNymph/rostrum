package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.DiffStats
import io.github.rhizonymph.rostrum.data.model.RankedFile
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.format.additionsText
import io.github.rhizonymph.rostrum.ui.format.countLabel
import io.github.rhizonymph.rostrum.ui.format.deletionsText
import io.github.rhizonymph.rostrum.ui.format.directoryOf
import io.github.rhizonymph.rostrum.ui.format.fileName
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The bar track behind a ranked file's +/− bar. */
private val BarTrack = Color(0xFF232733)

/** `7 files  +900  −9  3 added · 4 modified`. */
@Composable
fun SummaryStrip(stats: DiffStats, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(countLabel(stats.files, "file"), style = RostrumText.monoStrong13, color = colors.text)
        Text(additionsText(stats.additions), style = RostrumText.mono13, color = colors.successText)
        Text(deletionsText(stats.deletions), style = RostrumText.mono13, color = colors.dangerText)
        Text(summaryLine(stats), style = RostrumText.meta, color = colors.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** "Largest changes": every file, biggest first, each with a +/− bar. */
@Composable
fun RankedFilesCard(files: List<RankedFile>, onOpenFile: (Int) -> Unit, modifier: Modifier = Modifier) {
    RostrumCard(modifier.fillMaxWidth()) {
        files.forEachIndexed { index, file ->
            if (index > 0) CardDivider()
            RankedFileRow(file, onClick = { onOpenFile(file.fileIndex) })
        }
    }
}

@Composable
private fun RankedFileRow(file: RankedFile, onClick: () -> Unit) {
    val colors = RostrumTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(role = Role.Button, onClickLabel = "Open diff", onClick = onClick)
            .padding(start = 14.dp, end = 14.dp, top = 8.dp, bottom = 9.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterVertically),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Text(
                    fileName(file.path),
                    style = RostrumText.mono13.copy(lineHeight = RostrumText.meta.lineHeight * 0.95f),
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                val directory = directoryOf(file.path).removeSuffix("/")
                if (directory.isNotEmpty()) {
                    Text(directory, style = RostrumText.mono11, color = colors.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            Text(
                buildAnnotatedString {
                    withStyle(RostrumText.mono12.toSpanStyle().copy(color = colors.successText)) { append(additionsText(file.additions)) }
                    append(" ")
                    val deletionsColor = if (file.deletions == 0) colors.textSubtle else colors.dangerText
                    withStyle(RostrumText.mono12.toSpanStyle().copy(color = deletionsColor)) { append(deletionsText(file.deletions)) }
                },
                style = RostrumText.mono12.copy(fontWeight = FontWeight.Normal),
            )
        }
        BoxWithConstraints(
            Modifier
                .fillMaxWidth()
                .height(4.dp)
                .clip(RoundedCornerShape(2.dp))
                .background(BarTrack),
        ) {
            val bar = rankedBar(file, maxWidth.value)
            Row {
                Box(Modifier.width(bar.additions.dp).height(4.dp).background(colors.success))
                Box(Modifier.width(bar.deletions.dp).height(4.dp).background(colors.danger))
            }
        }
    }
}
