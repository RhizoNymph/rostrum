package io.github.rhizonymph.rostrum.ui.common

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome

/**
 * What a screen (or a part of one) shows while its data loads. Every
 * ViewModel exposes its content through one of these, so "loading", "failed"
 * and "here it is" can never be true at once.
 */
sealed interface UiState<out T> {
    data object Loading : UiState<Nothing>

    data class Loaded<out T>(val data: T) : UiState<T>

    data class Error(val error: BackendError) : UiState<Nothing>
}

fun <T> Outcome<T>.toUiState(): UiState<T> = when (this) {
    is Outcome.Ok -> UiState.Loaded(value)
    is Outcome.Err -> UiState.Error(error)
}

fun <T> UiState<T>.dataOrNull(): T? = (this as? UiState.Loaded)?.data

inline fun <T, R> UiState<T>.map(transform: (T) -> R): UiState<R> = when (this) {
    UiState.Loading -> UiState.Loading
    is UiState.Loaded -> UiState.Loaded(transform(data))
    is UiState.Error -> this
}

/** An action the user started that has not finished; drives spinners and disabled buttons. */
sealed interface ActionState {
    data object Idle : ActionState

    data object Running : ActionState

    data class Failed(val error: BackendError) : ActionState
}

val ActionState.running: Boolean get() = this == ActionState.Running
