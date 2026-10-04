package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The bar under a conversation (a pull request's or an issue's): a pill
 * comment field, sent with the button inside it or the keyboard's send key,
 * and [trailing] actions beside it (the pull request's Review button).
 */
@Composable
fun CommentBar(
    text: String,
    onTextChange: (String) -> Unit,
    onSend: () -> Unit,
    sending: Boolean,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    placeholder: String = "Add a comment",
    trailing: (@Composable RowScope.() -> Unit)? = null,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxWidth().background(colors.surface)) {
        CardDivider()
        Row(
            modifier = Modifier.fillMaxWidth().heightIn(min = 71.dp).padding(horizontal = 12.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val shape = RoundedCornerShape(22.dp)
            Row(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clip(shape)
                    .background(colors.raised)
                    .border(1.dp, colors.border, shape)
                    .padding(start = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(RostrumIcons.Comment, contentDescription = null, tint = colors.textMuted, modifier = Modifier.size(18.dp))
                Box(Modifier.weight(1f).padding(vertical = 10.dp)) {
                    if (text.isEmpty()) Text(placeholder, style = RostrumText.label.copy(fontWeight = FontWeight.Normal), color = colors.textMuted)
                    BasicTextField(
                        value = text,
                        onValueChange = onTextChange,
                        enabled = enabled && !sending,
                        maxLines = 5,
                        textStyle = RostrumText.label.copy(fontWeight = FontWeight.Normal, color = colors.text),
                        cursorBrush = SolidColor(colors.accent),
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Send),
                        keyboardActions = KeyboardActions(onSend = { onSend() }),
                        modifier = Modifier.fillMaxWidth().semantics { contentDescription = placeholder },
                    )
                }
                if (sending) {
                    Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
                    }
                } else if (text.isNotBlank()) {
                    RostrumIconButton(RostrumIcons.Send, "Send comment", onSend, tint = colors.accentText, iconSize = 20.dp)
                }
            }
            trailing?.invoke(this)
        }
    }
}
