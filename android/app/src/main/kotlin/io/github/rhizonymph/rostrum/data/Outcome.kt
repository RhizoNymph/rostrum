package io.github.rhizonymph.rostrum.data

/**
 * The result of a backend call: a value, or a typed [BackendError]. Errors are
 * part of every signature instead of exceptions, so a caller cannot forget to
 * handle one and never needs a catch-all.
 */
sealed interface Outcome<out T> {
    data class Ok<out T>(val value: T) : Outcome<T>

    data class Err(val error: BackendError) : Outcome<Nothing>
}

inline fun <T, R> Outcome<T>.map(transform: (T) -> R): Outcome<R> = when (this) {
    is Outcome.Ok -> Outcome.Ok(transform(value))
    is Outcome.Err -> this
}

inline fun <T, R> Outcome<T>.andThen(next: (T) -> Outcome<R>): Outcome<R> = when (this) {
    is Outcome.Ok -> next(value)
    is Outcome.Err -> this
}

inline fun <T> Outcome<T>.onOk(action: (T) -> Unit): Outcome<T> {
    if (this is Outcome.Ok) action(value)
    return this
}

inline fun <T> Outcome<T>.onErr(action: (BackendError) -> Unit): Outcome<T> {
    if (this is Outcome.Err) action(error)
    return this
}

fun <T> Outcome<T>.valueOrNull(): T? = (this as? Outcome.Ok)?.value

fun <T> Outcome<T>.errorOrNull(): BackendError? = (this as? Outcome.Err)?.error
