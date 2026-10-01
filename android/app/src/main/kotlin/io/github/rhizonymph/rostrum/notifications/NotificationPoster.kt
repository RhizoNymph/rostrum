package io.github.rhizonymph.rostrum.notifications

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.github.rhizonymph.rostrum.R
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.ui.navigation.AppLinks

/** Creates the channels and posts notifications that open the pull request in its profile. */
class NotificationPoster(private val context: Context) {
    fun ensureChannels() {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        RostrumChannel.entries.forEach { channel ->
            val importance = if (channel == RostrumChannel.ReviewRequests) {
                NotificationManager.IMPORTANCE_DEFAULT
            } else {
                NotificationManager.IMPORTANCE_LOW
            }
            manager.createNotificationChannel(
                NotificationChannel(channel.id, channel.title, importance).apply { description = channel.description },
            )
        }
    }

    /** Whether posting would show anything: the permission (API 33+) and the app-level switch. */
    fun canPost(): Boolean {
        val permitted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        return permitted && NotificationManagerCompat.from(context).areNotificationsEnabled()
    }

    fun post(spec: NotificationSpec) {
        if (!canPost()) {
            RostrumLog.i(TAG, "notification_suppressed", "pr" to spec.pr, "reason" to "not permitted")
            return
        }
        val tap = NotificationContent.tapExtras(spec)
        val open = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra(AppLinks.EXTRA_REPO, tap.repo)
            putExtra(AppLinks.EXTRA_NUMBER, tap.number)
            putExtra(AppLinks.EXTRA_PROFILE, tap.profile)
        }
        val pending = open?.let {
            PendingIntent.getActivity(context, spec.id, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val notification = NotificationCompat.Builder(context, spec.channel.id)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(spec.title)
            .setContentText(spec.text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(spec.text))
            .setAutoCancel(true)
            .setContentIntent(pending)
            .setCategory(NotificationCompat.CATEGORY_SOCIAL)
            .build()
        try {
            NotificationManagerCompat.from(context).notify(spec.id, notification)
            RostrumLog.i(TAG, "notification_posted", "pr" to spec.pr, "profile" to spec.profile, "channel" to spec.channel.id)
        } catch (e: SecurityException) {
            RostrumLog.w(TAG, "notification_denied", "pr" to spec.pr, "reason" to e.message)
        }
    }

    private companion object {
        const val TAG = "RostrumNotify"
    }
}
