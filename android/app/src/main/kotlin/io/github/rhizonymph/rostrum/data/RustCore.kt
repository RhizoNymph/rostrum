package io.github.rhizonymph.rostrum.data

import uniffi.rostrum_ffi.ffiVersion

/** Whether the Rust core (`librostrum_ffi.so`) loaded and answered. */
sealed interface CoreLink {
    data class Linked(val ffiVersion: String) : CoreLink

    /** The library or JNA's dispatcher could not be loaded or bound. */
    data class Unavailable(val reason: String) : CoreLink
}

/**
 * Entry point to the UniFFI bindings (package `uniffi.rostrum_ffi`, generated
 * at build time). FFI adapters for real features belong in this package.
 */
object RustCore {
    /**
     * Calls into the library once. The first call makes JNA load and bind it,
     * so this blocks; call it off the main thread.
     */
    fun probe(): CoreLink =
        try {
            CoreLink.Linked(ffiVersion())
        } catch (e: LinkageError) {
            // UnsatisfiedLinkError from JNA, or the ExceptionInInitializerError /
            // NoClassDefFoundError it becomes inside the bindings' initialiser.
            CoreLink.Unavailable(e.message ?: e.javaClass.name)
        }
}
