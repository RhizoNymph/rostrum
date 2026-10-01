package io.github.rhizonymph.rostrum.ui.common

import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.staticCompositionLocalOf
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.receiveAsFlow

/**
 * One-shot messages from a ViewModel to the snackbar ("Merged #10", "Copied").
 * Buffered, so a message sent while the screen is between collectors is shown
 * when it comes back rather than lost.
 */
class Messages {
    private val channel = Channel<String>(Channel.BUFFERED)
    val flow: Flow<String> = channel.receiveAsFlow()

    fun send(text: String) {
        channel.trySend(text)
    }
}

/** The app-wide snackbar host, provided by the root scaffold. */
val LocalSnackbarHostState = staticCompositionLocalOf { SnackbarHostState() }

/** Show every message from [messages] in the app's snackbar. */
@Composable
fun CollectMessages(messages: Flow<String>) {
    val host = LocalSnackbarHostState.current
    LaunchedEffect(messages, host) {
        messages.collect { host.showSnackbar(it, duration = SnackbarDuration.Short) }
    }
}
