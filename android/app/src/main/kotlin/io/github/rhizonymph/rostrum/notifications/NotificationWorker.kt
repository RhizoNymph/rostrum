package io.github.rhizonymph.rostrum.notifications

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import io.github.rhizonymph.rostrum.RostrumApplication
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.requiresSignIn
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.isSignedIn

/** What one background check decided; returned by [NotificationCheck.run] for tests. */
sealed interface CheckResult {
    data object SignedOut : CheckResult

    data class Posted(val count: Int) : CheckResult

    /** Worth trying again later (network, rate limit, desktop timeout). */
    data class Retry(val error: BackendError) : CheckResult

    /** Not worth retrying until something changes (bad token, a bug). */
    data class GaveUp(val error: BackendError) : CheckResult
}

/**
 * The periodic check, free of WorkManager: restore the session, ask the core
 * what is new, post it. The core already filters by the notification
 * settings and never reports your own pull requests.
 */
class NotificationCheck(
    private val session: SessionRepository,
    private val backend: RostrumBackend,
    private val post: (NotificationSpec) -> Unit,
) {
    suspend fun run(): CheckResult {
        session.restore()
        if (!session.state.value.isSignedIn) return CheckResult.SignedOut
        return when (val events = backend.checkNotifications()) {
            is Outcome.Ok -> {
                events.value.map(NotificationContent::of).forEach(post)
                CheckResult.Posted(events.value.size)
            }
            is Outcome.Err -> when {
                events.error.requiresSignIn -> CheckResult.GaveUp(events.error)
                events.error is BackendError.Network || events.error is BackendError.GitHubRateLimited -> CheckResult.Retry(events.error)
                else -> CheckResult.GaveUp(events.error)
            }
        }
    }
}

class NotificationWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val container = (applicationContext as RostrumApplication).container
        container.notificationPoster.ensureChannels()
        val result = NotificationCheck(container.session, container.backend, container.notificationPoster::post).run()
        RostrumLog.i(TAG, "notification_check", "result" to result::class.simpleName, "attempt" to runAttemptCount)
        return when (result) {
            CheckResult.SignedOut, is CheckResult.Posted, is CheckResult.GaveUp -> Result.success()
            is CheckResult.Retry -> Result.retry()
        }
    }

    private companion object {
        const val TAG = "RostrumNotify"
    }
}
