package io.github.rhizonymph.rostrum.ui.pr.common

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.components.CommentBar
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The bar under the Conversation and Checks tabs: the shared comment field
 * and the Review button, which carries the number of pending drafts.
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
    CommentBar(
        text = text,
        onTextChange = onTextChange,
        onSend = onSend,
        sending = sending,
        modifier = modifier,
        enabled = enabled,
    ) {
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
