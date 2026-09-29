package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.rostrum_ffi.RostrumCore
import java.io.File

/**
 * The process's one [RostrumCore], opened on first use. Loading the native
 * library and opening the core happen on [io]; a failure is returned (and
 * retried by the next call) rather than thrown.
 *
 * @param onOpened runs once, right after the core opens (the feed observer).
 */
class CoreHandle(
    private val dataDir: File,
    private val onOpened: suspend (RostrumCore) -> Unit = {},
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    private val mutex = Mutex()

    @Volatile
    private var core: RostrumCore? = null

    suspend fun get(): Outcome<RostrumCore> {
        core?.let { return Outcome.Ok(it) }
        return mutex.withLock {
            core?.let { return@withLock Outcome.Ok(it) }
            when (val opened = withContext(io) { open() }) {
                is Outcome.Err -> opened
                is Outcome.Ok -> {
                    core = opened.value
                    onOpened(opened.value)
                    RostrumLog.i(TAG, "core_opened", "dir" to dataDir.name)
                    opened
                }
            }
        }
    }

    private suspend fun open(): Outcome<RostrumCore> = try {
        if (!dataDir.isDirectory && !dataDir.mkdirs()) {
            Outcome.Err(BackendError.Storage("could not create ${dataDir.path}"))
        } else {
            FfiLogSink.installOnce()
            ffiCall("open") { RostrumCore.open(dataDir.absolutePath) }
        }
    } catch (e: LinkageError) {
        // UnsatisfiedLinkError from JNA, or the NoClassDefFoundError /
        // ExceptionInInitializerError it becomes inside the bindings.
        RostrumLog.e(TAG, "core_library_unavailable", "error" to e.javaClass.simpleName, "reason" to e.message)
        Outcome.Err(BackendError.Internal("the core library could not be loaded: ${e.message ?: e.javaClass.simpleName}"))
    }

    private companion object {
        const val TAG = "RostrumCore"
    }
}
