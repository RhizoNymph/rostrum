package io.github.rhizonymph.rostrum.notifications

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import io.github.rhizonymph.rostrum.data.RostrumLog
import java.util.concurrent.TimeUnit

/** The slice of WorkManager the scheduler needs, so its decisions are testable. */
interface BackgroundWork {
    /** Keep a periodic job named [name] scheduled (an existing one is kept as is). */
    fun schedulePeriodic(name: String)

    fun cancel(name: String)
}

/** Runs [NotificationWorker] every 15 minutes while the network is connected. */
class WorkManagerBackgroundWork(private val context: Context) : BackgroundWork {
    override fun schedulePeriodic(name: String) {
        val request = PeriodicWorkRequestBuilder<NotificationWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(name, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    override fun cancel(name: String) {
        WorkManager.getInstance(context).cancelUniqueWork(name)
    }
}

/**
 * Keeps the background check scheduled exactly when it can report something:
 * some profile is signed in with at least one notification toggle on (see
 * [io.github.rhizonymph.rostrum.data.profiles.ProfileManager.wantsNotifications]).
 */
class NotificationScheduler(private val work: BackgroundWork) {
    fun sync(wanted: Boolean) {
        if (wanted) work.schedulePeriodic(WORK_NAME) else work.cancel(WORK_NAME)
        RostrumLog.i(TAG, "notification_schedule", "scheduled" to wanted)
    }

    companion object {
        const val WORK_NAME = "rostrum.notifications.check"
        private const val TAG = "RostrumNotify"
    }
}
