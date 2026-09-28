package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The mockups' scrim: rgba(5,6,8,0.62). */
val SheetScrim = Color(0x9E050608)

/**
 * A modal bottom sheet in the app's style: #1c2029, top radius 28, a 32×4
 * handle. Always fully expanded. Content should add `imePadding()` when it
 * holds a text field.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RostrumBottomSheet(
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = RostrumTheme.colors
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        modifier = modifier,
        sheetState = sheetState,
        shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
        containerColor = colors.raised,
        contentColor = colors.text,
        scrimColor = SheetScrim,
        dragHandle = {
            Box(Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 4.dp), contentAlignment = Alignment.Center) {
                Box(Modifier.size(width = 32.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(colors.borderStrong))
            }
        },
        content = content,
    )
}

/**
 * The confirmation every irreversible action goes through (close, reopen,
 * abort). [destructive] paints the confirm button in the danger colour.
 */
@Composable
fun ConfirmDialog(
    title: String,
    body: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    destructive: Boolean = false,
) {
    val colors = RostrumTheme.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = colors.raised,
        titleContentColor = colors.text,
        textContentColor = colors.textSecondary,
        shape = RoundedCornerShape(28.dp),
        title = { Text(title, style = RostrumText.sheetTitle) },
        text = { Text(body, style = RostrumText.body) },
        confirmButton = {
            TextPillButton(
                confirmLabel,
                onClick = onConfirm,
                color = if (destructive) colors.dangerText else colors.accentText,
                style = RostrumText.button,
            )
        },
        dismissButton = { TextPillButton("Cancel", onClick = onDismiss) },
    )
}
