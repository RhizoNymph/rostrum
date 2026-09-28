package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * Pill buttons in the mockups' variants. All are Material buttons underneath,
 * so they keep the 48dp minimum touch target even when drawn 44dp tall.
 * [busy] swaps the label for a spinner and disables the button.
 */
@Composable
fun PrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    height: Dp = 44.dp,
    trailing: (@Composable RowScope.() -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    FilledPill(text, onClick, modifier, enabled, busy, height, colors.accent, colors.onAccent, trailing)
}

@Composable
fun TonalButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    height: Dp = 44.dp,
) {
    val colors = RostrumTheme.colors
    FilledPill(text, onClick, modifier, enabled, busy, height, colors.tonal, colors.onTonal, null)
}

/** The green merge button. */
@Composable
fun MergeButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    height: Dp = 44.dp,
    trailing: (@Composable RowScope.() -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    FilledPill(text, onClick, modifier, enabled, busy, height, colors.merge, colors.onMerge, trailing)
}

@Composable
private fun FilledPill(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier,
    enabled: Boolean,
    busy: Boolean,
    height: Dp,
    container: Color,
    content: Color,
    trailing: (@Composable RowScope.() -> Unit)?,
) {
    val colors = RostrumTheme.colors
    Button(
        onClick = onClick,
        enabled = enabled && !busy,
        modifier = modifier.height(height),
        shape = CircleShape,
        colors = ButtonDefaults.buttonColors(
            containerColor = container,
            contentColor = content,
            disabledContainerColor = if (busy) container else colors.raised,
            disabledContentColor = if (busy) content else colors.textSubtle,
        ),
        contentPadding = PaddingValues(horizontal = 20.dp),
    ) {
        PillContent(text, busy, content, RostrumText.button, trailing)
    }
}

/** Outlined pill: `Update: Merge`, `Open PR`, the four local operations. */
@Composable
fun OutlinedPillButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    height: Dp = 44.dp,
) {
    val colors = RostrumTheme.colors
    OutlinedButton(
        onClick = onClick,
        enabled = enabled && !busy,
        modifier = modifier.height(height),
        shape = CircleShape,
        border = BorderStroke(1.dp, if (enabled) colors.borderStrong else colors.border),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = colors.text,
            disabledContentColor = colors.textSubtle,
        ),
        contentPadding = PaddingValues(horizontal = 16.dp),
    ) {
        PillContent(text, busy, colors.text, RostrumText.label, null)
    }
}

/** Red-outlined pill: `Abort rebase`, `Close pull request`. */
@Composable
fun DangerOutlinedButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    height: Dp = 44.dp,
) {
    val colors = RostrumTheme.colors
    OutlinedButton(
        onClick = onClick,
        enabled = enabled && !busy,
        modifier = modifier.height(height),
        shape = CircleShape,
        border = BorderStroke(1.dp, colors.danger.copy(alpha = if (enabled) 0.5f else 0.25f)),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = colors.dangerText,
            disabledContentColor = colors.textSubtle,
        ),
        contentPadding = PaddingValues(horizontal = 20.dp),
    ) {
        PillContent(text, busy, colors.dangerText, RostrumText.label, null)
    }
}

/** Borderless accent-text button: `Clear`, `Sign out`, `Reply…`, `Cancel`. */
@Composable
fun TextPillButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    color: Color = RostrumTheme.colors.accentText,
    style: TextStyle = RostrumText.label,
    trailing: (@Composable RowScope.() -> Unit)? = null,
) {
    TextButton(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.height(44.dp),
        shape = CircleShape,
        colors = ButtonDefaults.textButtonColors(contentColor = color, disabledContentColor = RostrumTheme.colors.textSubtle),
        contentPadding = PaddingValues(horizontal = 12.dp),
    ) {
        PillContent(text, false, color, style, trailing)
    }
}

@Composable
private fun RowScope.PillContent(
    text: String,
    busy: Boolean,
    color: Color,
    style: TextStyle,
    trailing: (@Composable RowScope.() -> Unit)?,
) {
    Box(contentAlignment = Alignment.Center) {
        Text(text, style = style, color = if (busy) Color.Transparent else Color.Unspecified, maxLines = 1)
        if (busy) CircularProgressIndicator(color = color, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
    }
    if (trailing != null && !busy) trailing()
}

/** A 48dp icon-only button; [contentDescription] is required for accessibility. */
@Composable
fun RostrumIconButton(
    icon: ImageVector,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tint: Color = RostrumTheme.colors.textSecondary,
    iconSize: Dp = 22.dp,
) {
    IconButton(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.size(48.dp),
        colors = IconButtonDefaults.iconButtonColors(contentColor = tint, disabledContentColor = RostrumTheme.colors.textSubtle),
    ) {
        Icon(icon, contentDescription = contentDescription, modifier = Modifier.size(iconSize))
    }
}
