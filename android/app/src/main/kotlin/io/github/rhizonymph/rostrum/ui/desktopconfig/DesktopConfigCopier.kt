package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.valueOrNull

/** A preview with what copying would change on this phone, in words. */
data class DesktopConfigOffer(val preview: DesktopConfigPreview, val changes: List<String>)

/**
 * "Copy settings from the desktop", as both pairing and Settings run it: a
 * preview, then the copy followed by a feed refresh. The copy re-reads the
 * desktop's settings, so an old preview is never what gets applied.
 */
class DesktopConfigCopier(private val backend: RostrumBackend) {
    suspend fun preview(): Outcome<DesktopConfigOffer> =
        when (val preview = backend.desktopConfig().logErr(TAG, "desktop_config_preview_failed")) {
            is Outcome.Err -> preview
            is Outcome.Ok -> {
                val phone = backend.settings().valueOrNull()
                Outcome.Ok(DesktopConfigOffer(preview.value, DesktopConfigText.changeLines(preview.value, phone)))
            }
        }

    /**
     * Copy and refresh the feed. Returns the snackbar line. A failed refresh
     * does not undo the copy; the feed reports it when it next loads.
     */
    suspend fun copy(machine: String): Outcome<String> =
        when (val copied = backend.copyDesktopConfig().logErr(TAG, "desktop_config_copy_failed")) {
            is Outcome.Err -> copied
            is Outcome.Ok -> {
                val repos = copied.value.repos.size
                RostrumLog.i(TAG, "desktop_config_copied", "machine" to machine, "repos" to repos)
                backend.refreshFeed().logErr(TAG, "refresh_after_copy_failed")
                Outcome.Ok(DesktopConfigText.copiedMessage(machine, repos))
            }
        }

    private companion object {
        const val TAG = "RostrumDesktopConfig"
    }
}
