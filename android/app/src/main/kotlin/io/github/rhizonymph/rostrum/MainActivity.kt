package io.github.rhizonymph.rostrum

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.ui.app.RostrumApp
import io.github.rhizonymph.rostrum.ui.navigation.AppLinks
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The single activity, and the target of `rostrum://pair` deep links and
 * notification taps. Being `singleTop`, a link opened while running arrives
 * through [onNewIntent]; either way it is handed to the container's link
 * inbox, which the navigation host drains once it is ready.
 */
class MainActivity : ComponentActivity() {
    private val container get() = (application as RostrumApplication).container

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        // A recreated activity (rotation) still holds the launch intent; only
        // a fresh start should act on it.
        if (savedInstanceState == null) handle(intent)
        setContent {
            RostrumTheme {
                RostrumApp(container)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        if (intent == null) return
        val number = intent.getIntExtra(AppLinks.EXTRA_NUMBER, -1).takeIf { it > 0 }
        val link = AppLinks.parse(intent.action, intent.dataString, intent.getStringExtra(AppLinks.EXTRA_REPO), number)
            ?: return
        RostrumLog.i("RostrumLinks", "link_received", "kind" to link::class.simpleName)
        container.links.offer(link)
    }
}
