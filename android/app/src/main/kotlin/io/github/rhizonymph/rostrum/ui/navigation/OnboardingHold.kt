package io.github.rhizonymph.rostrum.ui.navigation

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Keeps the signed-out navigation graph on screen while a first-run pairing
 * still has a question to ask ("Copy settings from the desktop?"), although
 * pairing has already signed the app in with the desktop's token. The root
 * treats the session as signed out while this is held.
 */
class OnboardingHold {
    private val _held = MutableStateFlow(false)
    val held: StateFlow<Boolean> = _held.asStateFlow()

    fun hold() {
        _held.value = true
    }

    fun release() {
        _held.value = false
    }
}
