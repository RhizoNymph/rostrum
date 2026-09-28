package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts

/** The port of the desktop's pairing page (plain HTTP, the QR code and the Open in Rostrum link). */
const val PAIRING_PAGE_PORT = 8484

/** "http://<desktop>:8484/" with the address in mono, inside a sentence. */
fun pairingPageSentence(before: String, after: String): AnnotatedString = buildAnnotatedString {
    append(before)
    withStyle(SpanStyle(fontFamily = RostrumFonts.Mono)) { append("http://<desktop>:$PAIRING_PAGE_PORT/") }
    append(after)
}
