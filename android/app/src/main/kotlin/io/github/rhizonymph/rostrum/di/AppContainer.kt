package io.github.rhizonymph.rostrum.di

import android.app.Application
import android.os.Build
import io.github.rhizonymph.rostrum.data.ffi.FfiProfileRegistry
import io.github.rhizonymph.rostrum.data.profiles.LegacyStateWipe
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.secrets.AndroidKeystoreCipher
import io.github.rhizonymph.rostrum.data.secrets.EncryptedFileSecretStore
import io.github.rhizonymph.rostrum.notifications.NotificationPoster
import io.github.rhizonymph.rostrum.notifications.NotificationScheduler
import io.github.rhizonymph.rostrum.notifications.WorkManagerBackgroundWork
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.navigation.AppLinkInbox
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import java.io.File
import java.time.Clock

/**
 * The app's object graph, built once by [io.github.rhizonymph.rostrum.RostrumApplication].
 * ViewModels receive the pieces they need from here and from the active
 * profile (see [io.github.rhizonymph.rostrum.ui.common.profileViewModel]);
 * nothing reaches for it globally except the notification worker.
 */
class AppContainer(private val app: Application) {
    val clock: Clock = Clock.systemDefaultZone()

    /** Lives as long as the process; for work that must outlast a screen. */
    val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** How this phone introduces itself to a desktop when pairing. */
    val deviceName: String = listOf(Build.MANUFACTURER, Build.MODEL)
        .filter { !it.isNullOrBlank() }
        .joinToString(" ")
        .ifBlank { "Android phone" }

    /**
     * Every profile: the core's registry (the list in
     * `files/rostrum/profiles.json`, settings and cache per profile under
     * `files/rostrum/profiles/<id>`), and each profile's secrets under its id
     * in `no_backup/secrets/profiles`. The UI and the notification worker share
     * it. The single-profile builds' `files/core` and unkeyed secrets are
     * wiped on start.
     */
    val profiles = ProfileManager(
        registry = FfiProfileRegistry(File(app.filesDir, "rostrum")),
        secrets = EncryptedFileSecretStore(
            directory = File(app.noBackupFilesDir, "secrets/profiles"),
            cipher = AndroidKeystoreCipher(),
        ),
        deviceName = deviceName,
        scope = appScope,
        legacy = LegacyStateWipe(
            legacyCoreDir = File(app.filesDir, "core"),
            legacySecretsDir = File(app.noBackupFilesDir, "secrets"),
        ),
    )

    val links = AppLinkInbox()

    /** Snackbar messages that must outlive the screen (and the profile) that sent them. */
    val appMessages = Messages()
    val notificationPoster = NotificationPoster(app)
    val notificationScheduler = NotificationScheduler(WorkManagerBackgroundWork(app))

    /** Load the profiles and keep the background check's schedule in step with them. */
    fun start() {
        notificationPoster.ensureChannels()
        appScope.launch {
            profiles.start()
            combine(profiles.state, profiles.signedInProfiles) { state, signedIn ->
                (state as? ProfilesState.Ready)?.profiles?.map { it.id }?.toSet() to signedIn
            }
                .distinctUntilChanged()
                .collect { resyncNotifications() }
        }
    }

    private suspend fun resyncNotifications() {
        notificationScheduler.sync(wanted = profiles.wantsNotifications())
    }

    /** Settings calls this after either notification toggle of any profile changes. */
    fun onNotificationSettingsChanged() {
        appScope.launch { resyncNotifications() }
    }
}
