package io.github.rhizonymph.rostrum.notifications

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import io.github.rhizonymph.rostrum.data.RostrumLog

/** Whether the app may post notifications, and a way to ask. */
class NotificationPermissionState internal constructor(
    granted: Boolean,
    private val launch: () -> Unit,
) {
    var granted by mutableStateOf(granted)
        internal set

    /** Shows the system prompt (no-op below Android 13, where no prompt exists). */
    fun request() = launch()
}

private fun Context.hasNotificationPermission(): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

@Composable
fun rememberNotificationPermission(): NotificationPermissionState {
    val context = LocalContext.current
    var state: NotificationPermissionState? = null
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        state?.granted = granted
        RostrumLog.i("RostrumNotify", "notification_permission", "granted" to granted)
    }
    state = remember(launcher) {
        NotificationPermissionState(context.hasNotificationPermission()) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
    return state
}

/**
 * Ask for the notification permission once per install, at a moment the user
 * can see why: the first time the signed-in feed is on screen. Later changes
 * go through the Settings toggles.
 */
@Composable
fun RequestNotificationPermissionOnce(enabled: Boolean) {
    val context = LocalContext.current
    val permission = rememberNotificationPermission()
    LaunchedEffect(enabled) {
        if (!enabled || permission.granted) return@LaunchedEffect
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (prefs.getBoolean(KEY_ASKED, false)) return@LaunchedEffect
        prefs.edit().putBoolean(KEY_ASKED, true).apply()
        permission.request()
    }
}

private const val PREFS = "rostrum.ui"
private const val KEY_ASKED = "asked_notification_permission"
