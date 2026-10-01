package io.github.rhizonymph.rostrum.ui.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavHostController
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.di.AppContainer
import io.github.rhizonymph.rostrum.notifications.RequestNotificationPermissionOnce
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.LocalAppContainer
import io.github.rhizonymph.rostrum.ui.common.LocalSnackbarHostState
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.navigation.AppLink
import io.github.rhizonymph.rostrum.ui.navigation.AppLinkInbox
import io.github.rhizonymph.rostrum.ui.navigation.BottomNavBar
import io.github.rhizonymph.rostrum.ui.navigation.Destination
import io.github.rhizonymph.rostrum.ui.navigation.RostrumNavHost
import io.github.rhizonymph.rostrum.ui.navigation.TopLevel
import io.github.rhizonymph.rostrum.ui.navigation.navigateTopLevel
import io.github.rhizonymph.rostrum.ui.navigation.openPullRequest
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * Root of the UI. Waits for the session to be restored, then shows the
 * navigation graph, starting at the feed when signed in and at sign-in
 * otherwise. Signing in or out rebuilds the graph, so neither side's back
 * stack survives the switch.
 */
@Composable
fun RostrumApp(container: AppContainer) {
    val snackbar = remember { SnackbarHostState() }
    CompositionLocalProvider(
        LocalAppContainer provides container,
        LocalSnackbarHostState provides snackbar,
    ) {
        val session by container.session.state.collectAsStateWithLifecycle()
        val onboardingHeld by container.onboardingHold.held.collectAsStateWithLifecycle()
        CollectMessages(container.appMessages.flow)
        when (val state = session) {
            SessionState.Restoring -> Splash()
            is SessionState.Ready -> {
                // A first-run pairing that is still asking about copying settings
                // keeps the signed-out graph (and its Pair screen) on screen.
                val signedIn = state.github is GitHubAuth.SignedIn && !onboardingHeld
                key(signedIn) {
                    MainScaffold(rememberNavController(), signedIn, container.links, snackbar)
                }
            }
        }
    }
}

@Composable
private fun MainScaffold(
    navController: NavHostController,
    signedIn: Boolean,
    links: AppLinkInbox,
    snackbar: SnackbarHostState,
) {
    val colors = RostrumTheme.colors
    val shell = rostrumViewModel { ShellViewModel(it.backend, it.session.state) }
    val badge by shell.desktopBadge.collectAsStateWithLifecycle()
    val entry by navController.currentBackStackEntryAsState()
    val current = TopLevel.entries.firstOrNull { item ->
        entry?.destination?.hierarchy?.any { it.hasRoute(item.destination::class) } == true
    }
    val pending by links.pending.collectAsStateWithLifecycle()

    LaunchedEffect(pending) {
        val link = pending ?: return@LaunchedEffect
        when (link) {
            is AppLink.Pair -> navController.navigate(Destination.Pair(link.uri))
            is AppLink.OpenPullRequest -> if (signedIn) navController.openPullRequest(link.pr)
        }
        links.consume(link)
    }

    RequestNotificationPermissionOnce(enabled = signedIn && current == TopLevel.Feed)

    Scaffold(
        containerColor = colors.bg,
        contentColor = colors.text,
        bottomBar = {
            if (signedIn && current != null) {
                BottomNavBar(current, badge, onSelect = { navController.navigateTopLevel(it) })
            }
        },
        snackbarHost = {
            SnackbarHost(snackbar) { data ->
                Snackbar(
                    snackbarData = data,
                    shape = RoundedCornerShape(12.dp),
                    containerColor = colors.raised,
                    contentColor = colors.text,
                    actionColor = colors.accentText,
                )
            }
        },
    ) { padding ->
        RostrumNavHost(
            navController = navController,
            signedIn = signedIn,
            modifier = Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding),
        )
    }
}

@Composable
private fun Splash() {
    val colors = RostrumTheme.colors
    Box(Modifier.fillMaxSize().background(colors.bg), contentAlignment = Alignment.Center) {
        Text("rostrum", style = RostrumText.wordmark, color = colors.text)
    }
}
