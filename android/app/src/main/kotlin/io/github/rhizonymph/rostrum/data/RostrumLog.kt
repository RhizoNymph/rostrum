package io.github.rhizonymph.rostrum.data

import android.util.Log

/**
 * Structured logging: every message is `event=<name> key=value ...`, so logcat
 * lines grep and parse the same way. Values containing spaces are quoted.
 * Never pass a secret as a field.
 */
object RostrumLog {
    fun d(tag: String, event: String, vararg fields: Pair<String, Any?>) {
        Log.d(tag, format(event, fields))
    }

    fun i(tag: String, event: String, vararg fields: Pair<String, Any?>) {
        Log.i(tag, format(event, fields))
    }

    fun w(tag: String, event: String, vararg fields: Pair<String, Any?>) {
        Log.w(tag, format(event, fields))
    }

    fun e(tag: String, event: String, vararg fields: Pair<String, Any?>) {
        Log.e(tag, format(event, fields))
    }

    fun format(event: String, fields: Array<out Pair<String, Any?>>): String = buildString {
        append("event=").append(event)
        for ((key, value) in fields) {
            append(' ').append(key).append('=').append(quote(value?.toString() ?: "null"))
        }
    }

    private fun quote(value: String): String =
        if (value.isEmpty() || value.any { it.isWhitespace() || it == '"' || it == '=' }) {
            "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
        } else {
            value
        }
}

/** Log a failed [Outcome] with its error's variant, and pass it through. */
fun <T> Outcome<T>.logErr(tag: String, event: String, vararg fields: Pair<String, Any?>): Outcome<T> {
    if (this is Outcome.Err) {
        RostrumLog.w(tag, event, *fields, "error" to error::class.simpleName, "detail" to error.describe())
    }
    return this
}
