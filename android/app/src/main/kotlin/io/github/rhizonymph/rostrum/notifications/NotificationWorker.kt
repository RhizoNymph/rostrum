package io.github.rhizonymph.rostrum.notifications

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import io.github.rhizonymph.rostrum.RostrumApplication
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.requiresSignIn
import io.github.rhizonymph.rostrum.data.session.isSignedIn

/** What one background check decided for one profile. */
sealed interface ProfileCheck {
    /** No working GitHub token. */
    data object SignedOut : ProfileCheck

    /** Both notification toggles are off. */
    data object NotificationsOff : ProfileCheck

    data class Posted(val count: Int) : ProfileCheck

    /** Worth trying again later (network, rate limit). */
    data class Retry(val error: BackendError) : ProfileCheck

    /** Not worth retrying until something changes (bad token, a bug). */
    data class GaveUp(val error: BackendError) : ProfileCheck
}

/** What one background check decided, per profile; returned by [NotificationCheck.run] for tests. */
sealed interface CheckResult {
    /** The profiles could not be loaded. */
    data class Unavailable(val error: BackendError) : CheckResult

    /** [profiles] in the order they were checked (most recently used first). */
    data class Checked(val profiles: Map<ProfileId, ProfileCheck>) : CheckResult {
        val posted: Int get() = profiles.values.sumOf { (it as? ProfileCheck.Posted)?.count ?: 0 }

        /** Some profile hit a passing problem; WorkManager should run the check again soon. */
        val shouldRetry: Boolean get() = profiles.values.any { it is ProfileCheck.Retry }
    }
}

/**
 * The periodic check, free of WorkManager: load and restore every profile,
 * then ask each one that is signed in with a notification toggle on what is
 * new, and post it under that profile's name. The core already filters by
 * the notification settings and never reports your own pull requests.
 */
class NotificationCheck(
    private val profiles: ProfileManager,
    private val post: (NotificationSpec) -> Unit,
) {
    suspend fun run(): CheckResult {
        profiles.start()
        val ready = when (val state = profiles.state.value) {
            is ProfilesState.Ready -> state
            is ProfilesState.Unavailable -> return CheckResult.Unavailable(state.error)
            ProfilesState.Starting -> return CheckResult.Unavailable(BackendError.Internal("the profiles did not load"))
        }
        return CheckResult.Checked(ready.profiles.associate { it.id to check(it) })
    }

    private suspend fun check(profile: Profile): ProfileCheck {
        val handle = profiles.handle(profile.id)
        if (!handle.session.state.value.isSignedIn) return ProfileCheck.SignedOut
        val settings = when (val loaded = handle.backend.settings()) {
            is Outcome.Err -> return failed(loaded.error)
            is Outcome.Ok -> loaded.value
        }
        if (!settings.notifyNewPullRequests && !settings.notifyReviewRequests) return ProfileCheck.NotificationsOff
        return when (val events = handle.backend.checkNotifications()) {
            is Outcome.Ok -> {
                events.value.map { NotificationContent.of(it, profile) }.forEach(post)
                ProfileCheck.Posted(events.value.size)
            }
            is Outcome.Err -> failed(events.error)
        }
    }

    private fun failed(error: BackendError): ProfileCheck = when {
        error.requiresSignIn -> ProfileCheck.GaveUp(error)
        error is BackendError.Network || error is BackendError.GitHubRateLimited -> ProfileCheck.Retry(error)
        else -> ProfileCheck.GaveUp(error)
    }
}

class NotificationWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val container = (applicationContext as RostrumApplication).container
        container.notificationPoster.ensureChannels()
        val result = NotificationCheck(container.profiles, container.notificationPoster::post).run()
        return when (result) {
            is CheckResult.Unavailable -> {
                RostrumLog.w(TAG, "notification_check", "result" to "unavailable", "error" to result.error::class.simpleName)
                Result.success()
            }
            is CheckResult.Checked -> {
                RostrumLog.i(
                    TAG, "notification_check",
                    "profiles" to result.profiles.size,
                    "posted" to result.posted,
                    "retry" to result.shouldRetry,
                    "attempt" to runAttemptCount,
                )
                if (result.shouldRetry) Result.retry() else Result.success()
            }
        }
    }

    private companion object {
        const val TAG = "RostrumNotify"
    }
}
