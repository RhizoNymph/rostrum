package io.github.rhizonymph.rostrum.ui.pr.common

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.shape.CircleShape
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
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The bar under the Conversation and Checks tabs: a pill comment field (send
 * with the button inside it or the keyboard's send key) and the Review
 * button, which carries the number of pending drafts.
 */
@Composable
fun ComposerBar(
    text: String,
    onTextChange: (String) -> Unit,
    onSend: () -> Unit,
    sending: Boolean,
    pendingDrafts: Int,
    onReview: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
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
                    if (text.isEmpty()) Text("Add a comment", style = RostrumText.label.copy(fontWeight = FontWeight.Normal), color = colors.textMuted)
                    BasicTextField(
                        value = text,
                        onValueChange = onTextChange,
                        enabled = enabled && !sending,
                        maxLines = 5,
                        textStyle = RostrumText.label.copy(fontWeight = FontWeight.Normal, color = colors.text),
                        cursorBrush = SolidColor(colors.accent),
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Send),
                        keyboardActions = KeyboardActions(onSend = { onSend() }),
                        modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Add a comment" },
                    )
                }
                if (sending) {
                    Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
                    }
                } else if (text.isNotBlank()) {
                    RostrumIconButton(PrIcons.Send, "Send comment", onSend, tint = colors.accentText, iconSize = 20.dp)
                }
            }
            PrimaryButton(
                text = "Review",
                onClick = onReview,
                enabled = enabled,
                trailing = if (pendingDrafts > 0) {
                    { DraftCountBadge(pendingDrafts, Modifier.padding(start = 8.dp)) }
                } else {
                    null
                },
            )
        }
    }
}

/** The dark count bubble on the Review button: "3 pending". */
@Composable
fun DraftCountBadge(count: Int, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Box(
        modifier = modifier
            .sizeIn(minWidth = 22.dp)
            .height(22.dp)
            .clip(CircleShape)
            .background(colors.onAccent)
            .padding(horizontal = 6.dp)
            .semantics { contentDescription = "$count pending" },
        contentAlignment = Alignment.Center,
    ) {
        Text(count.toString(), style = RostrumText.mono11.copy(fontWeight = FontWeight.SemiBold), color = colors.accentText)
    }
}
