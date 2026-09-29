package io.github.rhizonymph.rostrum.di

import android.app.Application
import android.os.Build
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.ffi.FfiRostrumBackend
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.data.secrets.AndroidKeystoreCipher
import io.github.rhizonymph.rostrum.data.secrets.EncryptedFileSecretStore
import io.github.rhizonymph.rostrum.data.secrets.SecretStore
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isSignedIn
import io.github.rhizonymph.rostrum.data.valueOrNull
import io.github.rhizonymph.rostrum.notifications.NotificationPoster
import io.github.rhizonymph.rostrum.notifications.NotificationScheduler
import io.github.rhizonymph.rostrum.notifications.WorkManagerBackgroundWork
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.navigation.AppLinkInbox
import io.github.rhizonymph.rostrum.ui.navigation.OnboardingHold
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.distinctUntilChangedBy
import kotlinx.coroutines.launch
import java.io.File
import java.time.Clock

/**
 * The app's object graph, built once by [io.github.rhizonymph.rostrum.RostrumApplication].
 * ViewModels receive the pieces they need from here (see
 * [io.github.rhizonymph.rostrum.ui.common.rostrumViewModel]); nothing reaches
 * for it globally except the notification worker.
 */
class AppContainer(private val app: Application) {
    val clock: Clock = Clock.systemDefaultZone()

    /**
     * The Rust core, through the generated bindings. One per process: the
     * UI and the notification worker share it. It keeps its settings and
     * cache under `files/core`; secrets never go there.
     */
    val backend: RostrumBackend = FfiRostrumBackend(File(app.filesDir, "core"))

    val secrets: SecretStore = EncryptedFileSecretStore(
        directory = File(app.noBackupFilesDir, "secrets"),
        cipher = AndroidKeystoreCipher(),
    )

    /** How this phone introduces itself to the desktop when pairing. */
    val deviceName: String = listOf(Build.MANUFACTURER, Build.MODEL)
        .filter { !it.isNullOrBlank() }
        .joinToString(" ")
        .ifBlank { "Android phone" }

    val session = SessionRepository(backend, secrets, deviceName)
    val links = AppLinkInbox()

    /** Holds the first-run screens while pairing asks about copying settings. */
    val onboardingHold = OnboardingHold()

    /** Snackbar messages that must outlive the screen that sent them (pairing → feed). */
    val appMessages = Messages()
    val notificationPoster = NotificationPoster(app)
    val notificationScheduler = NotificationScheduler(WorkManagerBackgroundWork(app))

    /** Lives as long as the process; for work that must outlast a screen. */
    val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** Restore the session and keep the background check's schedule in step with it. */
    fun start() {
        notificationPoster.ensureChannels()
        appScope.launch {
            session.restore()
            session.state
                .distinctUntilChangedBy { it.isSignedIn }
                .collect { state -> resyncNotifications(state) }
        }
    }

    private suspend fun resyncNotifications(state: SessionState) {
        val settings = if (state.isSignedIn) backend.settings().valueOrNull() else null
        notificationScheduler.sync(
            signedIn = state.isSignedIn,
            notifyNewPullRequests = settings?.notifyNewPullRequests ?: false,
            notifyReviewRequests = settings?.notifyReviewRequests ?: false,
        )
    }

    /** Settings calls this after either notification toggle changes. */
    fun onNotificationSettingsChanged(settings: Settings) {
        notificationScheduler.sync(
            signedIn = session.state.value.isSignedIn,
            notifyNewPullRequests = settings.notifyNewPullRequests,
            notifyReviewRequests = settings.notifyReviewRequests,
        )
    }
}
