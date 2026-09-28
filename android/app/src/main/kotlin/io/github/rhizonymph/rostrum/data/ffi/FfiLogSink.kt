package io.github.rhizonymph.rostrum.data.ffi

import android.util.Log
import io.github.rhizonymph.rostrum.data.RostrumLog
import uniffi.rostrum_ffi.LogLevel
import uniffi.rostrum_ffi.LogRecord
import uniffi.rostrum_ffi.LogSink
import uniffi.rostrum_ffi.installLogSink
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Forwards the core's `tracing` records to logcat in the app's key=value
 * shape, under the tag `RostrumCore`. The core already redacts secrets.
 */
internal object FfiLogSink : LogSink {
    private val installed = AtomicBoolean(false)

    /** Install once per process (the core ignores later installs anyway). */
    fun installOnce() {
        if (installed.compareAndSet(false, true)) installLogSink(this, LogLevel.INFO)
    }

    override fun log(record: LogRecord) {
        Log.println(priorityOf(record.level), TAG, format(record))
    }

    fun priorityOf(level: LogLevel): Int = when (level) {
        LogLevel.ERROR -> Log.ERROR
        LogLevel.WARN -> Log.WARN
        LogLevel.INFO -> Log.INFO
        LogLevel.DEBUG -> Log.DEBUG
        LogLevel.TRACE -> Log.VERBOSE
    }

    fun format(record: LogRecord): String {
        val fields = listOf("target" to record.target, "msg" to record.message) +
            record.fields.map { it.key to it.value }
        return RostrumLog.format("core", fields.toTypedArray())
    }

    private const val TAG = "RostrumCore"
}
