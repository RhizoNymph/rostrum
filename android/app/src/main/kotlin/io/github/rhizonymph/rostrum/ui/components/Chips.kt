package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

private val ChipShape = RoundedCornerShape(6.dp)

/**
 * The 22dp status tag: `Your review`, `↓4 main`, `conflict`, `Draft`.
 * [mono] sets the text in JetBrains Mono (counts and refs). Neutral chips get
 * the raised fill with a border, as in the mockups.
 */
@Composable
fun StatusChip(
    text: String,
    role: ColorRole,
    modifier: Modifier = Modifier,
    mono: Boolean = false,
    leadingIcon: ImageVector? = null,
    description: String? = null,
) {
    val colors = role.colors()
    val border = if (role == ColorRole.Neutral) {
        Modifier.border(1.dp, RostrumTheme.colors.border, ChipShape)
    } else {
        Modifier
    }
    Row(
        modifier = modifier
            .height(22.dp)
            .clip(ChipShape)
            .background(colors.tint)
            .then(border)
            .padding(horizontal = 8.dp)
            .then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (leadingIcon != null) {
            Icon(leadingIcon, contentDescription = null, tint = colors.text, modifier = Modifier.size(12.dp))
        }
        Text(
            text = text,
            style = if (mono) RostrumText.chip.copy(fontFamily = RostrumFonts.Mono) else RostrumText.chip,
            color = colors.text,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** A core-provided [Chip]. */
@Composable
fun StatusChip(chip: Chip, modifier: Modifier = Modifier, mono: Boolean = false, leadingIcon: ImageVector? = null) {
    StatusChip(chip.text, chip.role, modifier, mono, leadingIcon, chip.tooltip?.let { "${chip.text}: $it" })
}

/** A GitHub label in its own colour (neutral when it has none). */
@Composable
fun LabelChip(label: LabelView, modifier: Modifier = Modifier) {
    val palette = RostrumTheme.colors
    val colors = labelColors(label.argb, palette)
    val border = if (label.argb == null) Modifier.border(1.dp, palette.border, ChipShape) else Modifier
    Row(
        modifier = modifier
            .height(22.dp)
            .clip(ChipShape)
            .background(colors.tint)
            .then(border)
            .padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label.name, style = RostrumText.chip, color = colors.text, maxLines = 1)
    }
}

/**
 * The 32dp filter chip under the feed header. Selected: tonal fill with a
 * check. Unselected: outlined.
 */
@Composable
fun FilterPill(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(8.dp)
    Row(
        modifier = modifier
            .height(32.dp)
            .clip(shape)
            .then(
                if (selected) Modifier.background(colors.tonal) else Modifier.border(1.dp, colors.borderStrong, shape),
            )
            .clickable(role = Role.Checkbox, onClick = onClick)
            .padding(start = if (selected) 8.dp else 12.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (selected) {
            Icon(RostrumIcons.Check, contentDescription = null, tint = colors.onTonal, modifier = Modifier.size(16.dp))
        }
        Text(
            text,
            style = RostrumText.label.copy(fontSize = RostrumText.meta.fontSize),
            color = if (selected) colors.onTonal else colors.textSecondary,
            maxLines = 1,
        )
    }
}

/** A small mono tag: commit SHAs in the timeline, `head 136c158`. */
@Composable
fun ShaTag(sha: String, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Text(
        text = sha.take(7),
        style = RostrumText.mono11,
        color = colors.textMuted,
        modifier = modifier
            .clip(RoundedCornerShape(5.dp))
            .background(colors.raised)
            .padding(horizontal = 6.dp, vertical = 1.dp),
    )
}

/** A branch name in a bordered mono box: `feat/diff-overview → main`. */
@Composable
fun RefTag(ref: String, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(6.dp)
    Text(
        text = ref,
        style = RostrumText.mono12,
        color = colors.textSecondary,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier
            .clip(shape)
            .background(colors.raised)
            .border(1.dp, colors.border, shape)
            .padding(horizontal = 8.dp, vertical = 2.dp),
    )
}
