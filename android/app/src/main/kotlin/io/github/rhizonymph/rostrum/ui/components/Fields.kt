package io.github.rhizonymph.rostrum.ui.components

import android.content.ClipData
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import kotlinx.coroutines.launch

/**
 * The mockups' text field: filled, bordered, radius 12, accent border when
 * focused and danger border with [errorText] for [isError]. [mono] sets the
 * text in JetBrains Mono (repositories, tokens, hosts, codes).
 */
@Composable
fun RostrumTextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    placeholder: String? = null,
    accessibilityLabel: String? = placeholder,
    mono: Boolean = false,
    isError: Boolean = false,
    errorText: String? = null,
    singleLine: Boolean = true,
    minHeight: Dp = 48.dp,
    enabled: Boolean = true,
    fill: Color = RostrumTheme.colors.surface,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    visualTransformation: VisualTransformation = VisualTransformation.None,
    textStyle: TextStyle = if (mono) RostrumText.mono13.copy(fontSize = RostrumText.label.fontSize) else RostrumText.body,
) {
    val colors = RostrumTheme.colors
    val interaction = remember { MutableInteractionSource() }
    val focused by interaction.collectIsFocusedAsState()
    val shape = RoundedCornerShape(12.dp)
    val borderColor = when {
        isError -> colors.danger
        focused -> colors.accent
        else -> colors.borderStrong
    }
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier.semantics {
            if (accessibilityLabel != null) contentDescription = accessibilityLabel
            if (isError && errorText != null) error(errorText)
        },
        enabled = enabled,
        singleLine = singleLine,
        textStyle = textStyle.copy(color = if (enabled) colors.text else colors.textSubtle),
        cursorBrush = SolidColor(colors.accent),
        keyboardOptions = keyboardOptions,
        keyboardActions = keyboardActions,
        visualTransformation = visualTransformation,
        interactionSource = interaction,
        decorationBox = { inner ->
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = minHeight)
                    .clip(shape)
                    .background(fill)
                    .border(if (focused || isError) 1.5.dp else 1.dp, borderColor, shape)
                    .padding(horizontal = 14.dp, vertical = if (singleLine) 0.dp else 10.dp),
                contentAlignment = if (singleLine) Alignment.CenterStart else Alignment.TopStart,
            ) {
                if (value.isEmpty() && placeholder != null) {
                    Text(
                        placeholder,
                        style = textStyle,
                        color = colors.textSubtle,
                        maxLines = if (singleLine) 1 else Int.MAX_VALUE,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                inner()
            }
        },
    )
}

/**
 * A command in a dark mono box with a copy button: the tmux attach command.
 * [wrap] breaks long commands over lines instead of eliding them.
 */
@Composable
fun CopyCommandRow(
    command: String,
    modifier: Modifier = Modifier,
    wrap: Boolean = false,
    copyDescription: String = "Copy attach command",
    onCopied: () -> Unit = {},
) {
    val colors = RostrumTheme.colors
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    val shape = RoundedCornerShape(10.dp)
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .clip(shape)
            .background(colors.bg)
            .border(1.dp, colors.border, shape)
            .padding(start = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = command,
            style = RostrumText.mono12.copy(fontFamily = RostrumFonts.Mono),
            color = colors.textSecondary,
            maxLines = if (wrap) Int.MAX_VALUE else 1,
            overflow = if (wrap) TextOverflow.Clip else TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).padding(vertical = 8.dp),
        )
        RostrumIconButton(
            icon = RostrumIcons.Copy,
            contentDescription = copyDescription,
            onClick = {
                scope.launch {
                    clipboard.setClipEntry(ClipEntry(ClipData.newPlainText("command", command)))
                    onCopied()
                }
            },
            tint = colors.accentText,
            iconSize = 18.dp,
        )
    }
}
