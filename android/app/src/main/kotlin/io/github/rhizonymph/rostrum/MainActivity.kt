package io.github.rhizonymph.rostrum

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import io.github.rhizonymph.rostrum.ui.RostrumApp
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * The single activity. Also the target of `rostrum://pair` deep links (see the
 * manifest); being `singleTop`, a link opened while running arrives through
 * [onNewIntent].
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        setContent {
            RostrumTheme {
                RostrumApp()
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Keep `intent` current so pairing (not built yet) reads the latest link.
        setIntent(intent)
    }
}
