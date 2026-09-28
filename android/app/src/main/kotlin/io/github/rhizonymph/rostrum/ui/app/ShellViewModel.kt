package io.github.rhizonymph.rostrum.ui.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isPaired
import io.github.rhizonymph.rostrum.data.valueOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn

/** State of the app chrome: the Desktop tab's "handoffs waiting" badge. */
class ShellViewModel(
    backend: RostrumBackend,
    session: StateFlow<SessionState>,
    pollMillis: Long = 60_000,
) : ViewModel() {
    @OptIn(ExperimentalCoroutinesApi::class)
    val desktopBadge: StateFlow<Int> = session
        .map { it.isPaired }
        .distinctUntilChanged()
        .flatMapLatest { paired -> if (paired) poll(backend, pollMillis) else flowOf(0) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), 0)

    private fun poll(backend: RostrumBackend, pollMillis: Long): Flow<Int> = flow {
        while (true) {
            emit(backend.handoffs().valueOrNull()?.size ?: 0)
            delay(pollMillis)
        }
    }
}
