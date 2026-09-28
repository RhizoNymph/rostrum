package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome

/*
 * "Copy settings from the desktop" over the core: `RostrumCore.desktopConfig()`
 * and `copyDesktopConfig()`. The generated bindings gain them with the next
 * core merge; until then both calls answer this error, and the mapping of
 * `DesktopConfigPreview` lands here with them.
 */
internal fun <T> desktopConfigUnavailable(): Outcome<T> =
    Outcome.Err(BackendError.Internal("copying settings from the desktop needs a newer core"))
