package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.ConfigPushResult
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.Settings

/**
 * Sharing settings with the paired desktop, both ways. All three answer
 * [BackendError.NotPaired] without a desktop.
 */
interface DesktopConfigApi {
    /** The desktop's settings, its revision, and the difference each way. */
    suspend fun desktopConfig(): Outcome<DesktopConfigPreview>

    /**
     * Replace this phone's repositories, pull requests and issues per
     * repository, feed preferences, sorts, trunks and stash default with the
     * desktop's (fetched afresh, never a stale preview) and persist them.
     * Refresh the feed afterwards.
     */
    suspend fun copyDesktopConfig(): Outcome<Settings>

    /**
     * Replace the desktop's shareable settings with this phone's, provided
     * they are still at [base] (the preview's revision). On
     * [ConfigPushResult.Changed] nothing was written.
     */
    suspend fun pushConfigToDesktop(base: String): Outcome<ConfigPushResult>
}
