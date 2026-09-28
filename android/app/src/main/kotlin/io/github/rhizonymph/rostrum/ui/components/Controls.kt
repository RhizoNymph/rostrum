package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The mockups' 52×32 switch. Visual only: put it inside a [SwitchRow] (or any
 * row made `toggleable`) so the whole row is the touch target.
 */
@Composable
fun RostrumSwitchVisual(checked: Boolean, enabled: Boolean = true, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val track by animateColorAsState(if (checked) colors.accent else colors.border, label = "track")
    val thumbSize by animateDpAsState(if (checked) 24.dp else 16.dp, label = "thumbSize")
    val thumbOffset by animateDpAsState(if (checked) 24.dp else 7.dp, label = "thumbOffset")
    Box(
        modifier = modifier
            .size(width = 52.dp, height = 32.dp)
            .clip(CircleShape)
            .background(if (enabled) track else track.copy(alpha = 0.4f))
            .then(if (checked) Modifier else Modifier.border(1.dp, colors.borderStrong, CircleShape)),
        contentAlignment = Alignment.CenterStart,
    ) {
        Box(
            Modifier
                .offset(x = if (checked) thumbOffset else thumbOffset - 1.dp)
                .size(thumbSize)
                .clip(CircleShape)
                .background(if (checked) colors.onAccent else colors.textSubtle),
        )
    }
}

/** A settings row with a title, optional subtitle and a switch; the whole row toggles. */
@Composable
fun SwitchRow(
    title: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    enabled: Boolean = true,
    horizontalPadding: Dp = 16.dp,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = if (subtitle == null) 56.dp else 72.dp)
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onCheckedChange)
            .padding(horizontal = horizontalPadding, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = RostrumText.rowTitle, color = if (enabled) colors.text else colors.textSubtle)
            if (subtitle != null) Text(subtitle, style = RostrumText.meta, color = colors.textMuted)
        }
        RostrumSwitchVisual(checked, enabled)
    }
}

/** The 20dp checkbox square of the author list. Visual only; make the row toggleable. */
@Composable
fun CheckboxVisual(checked: Boolean, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(5.dp)
    Box(
        modifier = modifier
            .size(20.dp)
            .clip(shape)
            .then(if (checked) Modifier.background(colors.accent) else Modifier.border(2.dp, colors.textSubtle, shape)),
        contentAlignment = Alignment.Center,
    ) {
        if (checked) Icon(RostrumIcons.CheckBold, contentDescription = null, tint = colors.onAccent, modifier = Modifier.size(14.dp))
    }
}

/** A 20dp radio circle. Visual only; make the row selectable. */
@Composable
fun RadioVisual(selected: Boolean, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Box(
        modifier = modifier
            .size(20.dp)
            .clip(CircleShape)
            .border(2.dp, if (selected) colors.accent else colors.textSubtle, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        if (selected) Box(Modifier.size(10.dp).clip(CircleShape).background(colors.accent))
    }
}

/**
 * A segmented control (`Merge commit | Squash | Rebase`, `Overview | Diff`,
 * `Write | Preview`): outlined pill, tonal fill and a check on the selection.
 */
@Composable
fun <T> SegmentedToggle(
    options: List<T>,
    selected: T,
    onSelect: (T) -> Unit,
    label: (T) -> String,
    modifier: Modifier = Modifier,
    height: Dp = 40.dp,
    showCheck: Boolean = true,
) {
    val colors = RostrumTheme.colors
    val shape = RoundedCornerShape(height / 2)
    Row(
        modifier = modifier
            .height(height)
            .clip(shape)
            .border(1.dp, colors.borderStrong, shape),
    ) {
        options.forEachIndexed { index, option ->
            val isSelected = option == selected
            if (index > 0) Box(Modifier.width(1.dp).fillMaxHeight().background(colors.borderStrong))
            Row(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .background(if (isSelected) colors.tonal else colors.bg.copy(alpha = 0f))
                    .selectable(selected = isSelected, role = Role.Tab, onClick = { onSelect(option) }),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (isSelected && showCheck) {
                    Icon(
                        RostrumIcons.Check,
                        contentDescription = null,
                        tint = colors.onTonal,
                        modifier = Modifier.padding(end = 6.dp).size(14.dp),
                    )
                }
                Text(
                    label(option),
                    style = RostrumText.label.copy(fontSize = RostrumText.meta.fontSize),
                    color = if (isSelected) colors.onTonal else colors.textSecondary,
                    maxLines = 1,
                )
            }
        }
    }
}

/** A tappable list row with a chevron, used for "Desktop · nymph-desk ›". */
@Composable
fun ChevronRow(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val colors = RostrumTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 60.dp)
            .clickable(onClick = onClick)
            .padding(start = 14.dp, end = 6.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(Modifier.weight(1f)) { content() }
        Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
            Icon(RostrumIcons.ChevronRight, contentDescription = null, tint = colors.textMuted, modifier = Modifier.size(18.dp))
        }
    }
}

/** A simple clickable row wrapper with the 48dp minimum height. */
fun Modifier.rowClick(onClick: () -> Unit): Modifier = this.heightIn(min = 48.dp).clickable(onClick = onClick)
