package io.github.rhizonymph.rostrum.ui.items

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.ui.components.CiGlyph
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.ciShape
import io.github.rhizonymph.rostrum.ui.components.colors
import io.github.rhizonymph.rostrum.ui.format.additionsText
import io.github.rhizonymph.rostrum.ui.format.deletionsText
import io.github.rhizonymph.rostrum.ui.theme.RostrumColors
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/**
 * One pull request: CI glyph, title, `#10 · ada-lin · 2h` with `+900 −9`
 * (or the Draft chip) on the right, then its chips. A stack member
 * ([stack] set) is indented behind a chain glyph.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PrRow(
    pr: PrSummary,
    now: Instant,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    stack: StackPlace? = null,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .clickable(onClick = onClick)
            .padding(start = if (stack == null) 14.dp else 8.dp, end = 14.dp, top = 13.dp, bottom = 13.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (stack != null) ChainGlyph(stack)
        CiGlyph(
            shape = pr.checks.ciShape(),
            color = pr.checksRole.colors().solid,
            modifier = Modifier.padding(top = 1.dp),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(
                pr.title,
                style = RostrumText.rowTitle,
                color = if (pr.isDraft) colors.textSecondary else colors.text,
            )
            MetaLine(pr, now, colors)
            val chips = rowChips(pr)
            if (chips.isNotEmpty()) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    chips.forEach { chip ->
                        StatusChip(
                            text = chip.text,
                            role = chip.role,
                            mono = chip.mono,
                            leadingIcon = when (chip.icon) {
                                ChipIcon.Check -> RostrumIcons.CheckBold
                                null -> null
                            },
                            description = chip.description?.let { "${chip.text}: $it" },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun MetaLine(pr: PrSummary, now: Instant, colors: RostrumColors) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text("#${pr.number}", style = RostrumText.mono12, color = colors.textMuted)
        Dot(colors)
        Text(authorLabel(pr), style = RostrumText.meta, color = colors.textMuted, maxLines = 1)
        Dot(colors)
        Text(ageLabel(pr, now), style = RostrumText.meta, color = colors.textMuted)
        Spacer(Modifier.weight(1f))
        if (pr.isDraft) {
            StatusChip("Draft", ColorRole.Draft)
        } else {
            Text(
                text = buildAnnotatedString {
                    withStyle(SpanStyle(color = toneColor(additionsTone(pr.additions), colors))) { append(additionsText(pr.additions)) }
                    append(" ")
                    withStyle(SpanStyle(color = toneColor(deletionsTone(pr.deletions), colors))) { append(deletionsText(pr.deletions)) }
                },
                style = RostrumText.mono12,
            )
        }
    }
}

/** The chain glyph of a stack member, with the rule joining it to the members above and below. */
@Composable
private fun ChainGlyph(place: StackPlace) {
    val colors = RostrumTheme.colors
    Box(Modifier.width(18.dp).fillMaxHeight().clearAndSetSemantics { contentDescription = "Stack member" }) {
        val up = place == StackPlace.Middle || place == StackPlace.Top
        val down = place == StackPlace.Middle || place == StackPlace.Bottom
        if (up) Box(Modifier.align(Alignment.TopCenter).width(1.dp).height(10.dp).offset(y = (-13).dp).background(colors.borderStrong))
        if (down) Box(Modifier.align(Alignment.BottomCenter).width(1.dp).fillMaxHeight(0.7f).offset(y = 13.dp).background(colors.borderStrong))
        Icon(RostrumIcons.Link, contentDescription = null, tint = colors.textMuted, modifier = Modifier.align(Alignment.TopCenter).size(14.dp).padding(top = 1.dp))
    }
}

@Composable
internal fun Dot(colors: RostrumColors) {
    Text("·", style = RostrumText.meta, color = colors.textMuted)
}

internal fun toneColor(tone: CountTone, colors: RostrumColors): Color = when (tone) {
    CountTone.Added -> colors.successText
    CountTone.Removed -> colors.dangerText
    CountTone.Zero -> colors.textSubtle
}
