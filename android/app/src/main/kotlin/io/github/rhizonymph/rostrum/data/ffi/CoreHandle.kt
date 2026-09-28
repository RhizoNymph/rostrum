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

/** Opens one [RostrumCore]: from a data directory, or a profile's from the registry. */
typealias CoreOpener = suspend () -> Outcome<RostrumCore>

/**
 * One [RostrumCore], opened on first use by [open]. Loading the native
 * library and opening the core happen on [io]; a failure is returned (and
 * retried by the next call) rather than thrown.
 *
 * @param name says which core in logs (a directory or a profile id).
 * @param onOpened runs once, right after the core opens (the feed observer).
 */
class CoreHandle(
    private val name: String,
    private val open: CoreOpener,
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
            when (val opened = withContext(io) { withNativeLibrary(open) }) {
                is Outcome.Err -> opened
                is Outcome.Ok -> {
                    core = opened.value
                    onOpened(opened.value)
                    RostrumLog.i(TAG, "core_opened", "core" to name)
                    opened
                }
            }
        }
    }

    companion object {
        private const val TAG = "RostrumCore"

        /** A standalone core keeping its data in [dataDir] (tests; the registry owns the app's). */
        fun inDirectory(dataDir: File): CoreOpener = {
            if (!dataDir.isDirectory && !dataDir.mkdirs()) {
                Outcome.Err(BackendError.Storage("could not create ${dataDir.path}"))
            } else {
                ffiCall("open") { RostrumCore.open(dataDir.absolutePath) }
            }
        }

        /**
         * Run [block], which loads the native library on first use, with the
         * core's log sink installed. A library that cannot be loaded becomes
         * an [BackendError.Internal] instead of a crash.
         */
        internal suspend fun <T> withNativeLibrary(block: suspend () -> Outcome<T>): Outcome<T> = try {
            FfiLogSink.installOnce()
            block()
        } catch (e: LinkageError) {
            // UnsatisfiedLinkError from JNA, or the NoClassDefFoundError /
            // ExceptionInInitializerError it becomes inside the bindings.
            RostrumLog.e(TAG, "core_library_unavailable", "error" to e.javaClass.simpleName, "reason" to e.message)
            Outcome.Err(BackendError.Internal("the core library could not be loaded: ${e.message ?: e.javaClass.simpleName}"))
        }
    }
}
