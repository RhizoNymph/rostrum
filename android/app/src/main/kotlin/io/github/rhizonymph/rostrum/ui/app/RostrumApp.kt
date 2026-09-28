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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavHostController
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.profiles.ProfileHandle
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isSignedIn
import io.github.rhizonymph.rostrum.di.AppContainer
import io.github.rhizonymph.rostrum.notifications.RequestNotificationPermissionOnce
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.LocalAppContainer
import io.github.rhizonymph.rostrum.ui.common.LocalProfileHandle
import io.github.rhizonymph.rostrum.ui.common.LocalSnackbarHostState
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.navigation.AppLink
import io.github.rhizonymph.rostrum.ui.navigation.AppLinkInbox
import io.github.rhizonymph.rostrum.ui.navigation.BottomNavBar
import io.github.rhizonymph.rostrum.ui.navigation.Destination
import io.github.rhizonymph.rostrum.ui.navigation.LinkRoute
import io.github.rhizonymph.rostrum.ui.navigation.RostrumNavHost
import io.github.rhizonymph.rostrum.ui.navigation.TopLevel
import io.github.rhizonymph.rostrum.ui.navigation.navigateTopLevel
import io.github.rhizonymph.rostrum.ui.navigation.openPullRequest
import io.github.rhizonymph.rostrum.ui.navigation.routeOf
import io.github.rhizonymph.rostrum.ui.profiles.ProfileSwitcherSheet
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import kotlinx.coroutines.launch

/**
 * Root of the UI. Waits for the profiles, then shows the active profile's
 * navigation graph: its feed when signed in, sign-in otherwise, and the
 * first-run sign-in when there is no profile. The graph is keyed by the
 * profile and its sign-in, with its ViewModels in a store of their own
 * ([GraphViewModelStores]): switching profiles (or signing in or out) starts
 * over at the new graph's first screen, with nothing of the old one running.
 */
@Composable
fun RostrumApp(container: AppContainer) {
    val snackbar = remember { SnackbarHostState() }
    CompositionLocalProvider(
        LocalAppContainer provides container,
        LocalSnackbarHostState provides snackbar,
    ) {
        val profiles by container.profiles.state.collectAsStateWithLifecycle()
        val stores = viewModel { GraphViewModelStores() }
        CollectMessages(container.appMessages.flow)
        when (val state = profiles) {
            ProfilesState.Starting -> Splash()
            is ProfilesState.Unavailable -> ProfilesUnavailable(state, container)
            is ProfilesState.Ready -> {
                FollowNotificationProfile(container, state)
                val active = state.active
                if (active == null) {
                    ProfileGraph(GraphKey(null, signedIn = false), null, null, hasProfiles = false, stores, container.links, snackbar)
                } else {
                    val handle = remember(active) { container.profiles.handle(active) }
                    val session by handle.session.state.collectAsStateWithLifecycle()
                    when (val current = session) {
                        SessionState.Restoring -> Splash()
                        is SessionState.Ready -> ProfileGraph(
                            key = GraphKey(active.value, current.isSignedIn),
                            handle = handle,
                            label = state.activeProfile?.label,
                            hasProfiles = true,
                            stores = stores,
                            links = container.links,
                            snackbar = snackbar,
                        )
                    }
                }
            }
        }
    }
}

/** A notification for another profile: switch to it first; its graph then opens the pull request. */
@Composable
private fun FollowNotificationProfile(container: AppContainer, state: ProfilesState.Ready) {
    val pending by container.links.pending.collectAsStateWithLifecycle()
    LaunchedEffect(pending, state.active) {
        val link = pending ?: return@LaunchedEffect
        when (val route = routeOf(link, state)) {
            LinkRoute.Show -> Unit
            is LinkRoute.SwitchFirst -> when (val switched = container.profiles.switchTo(route.profile)) {
                is Outcome.Ok -> Unit
                is Outcome.Err -> {
                    container.links.consume(link)
                    container.appMessages.send("Couldn't switch profiles: ${switched.error.describe()}")
                }
            }
            LinkRoute.ProfileGone -> {
                container.links.consume(link)
                container.appMessages.send("That notification's profile was removed from this phone")
            }
        }
    }
}

@Composable
private fun ProfileGraph(
    key: GraphKey,
    handle: ProfileHandle?,
    label: String?,
    hasProfiles: Boolean,
    stores: GraphViewModelStores,
    links: AppLinkInbox,
    snackbar: SnackbarHostState,
) {
    key(key) {
        val owner = remember(key) { stores.ownerFor(key) }
        CompositionLocalProvider(
            LocalViewModelStoreOwner provides owner,
            LocalProfileHandle provides handle,
        ) {
            MainScaffold(rememberNavController(), key.signedIn, handle, label, hasProfiles, links, snackbar)
        }
    }
}

@Composable
private fun MainScaffold(
    navController: NavHostController,
    signedIn: Boolean,
    handle: ProfileHandle?,
    label: String?,
    hasProfiles: Boolean,
    links: AppLinkInbox,
    snackbar: SnackbarHostState,
) {
    val colors = RostrumTheme.colors
    val badge = if (signedIn) {
        val shell = profileViewModel { _, profile -> ShellViewModel(profile.backend, profile.session.state) }
        shell.desktopBadge.collectAsStateWithLifecycle().value
    } else {
        0
    }
    val entry by navController.currentBackStackEntryAsState()
    val current = TopLevel.entries.firstOrNull { item ->
        entry?.destination?.hierarchy?.any { it.hasRoute(item.destination::class) } == true
    }
    val pending by links.pending.collectAsStateWithLifecycle()
    var switcherOpen by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(pending) {
        val link = pending ?: return@LaunchedEffect
        when (link) {
            is AppLink.Pair -> navController.navigate(Destination.Pair(link.uri))
            is AppLink.OpenPullRequest -> {
                // Another profile's notification: the root switches first, then this graph is replaced.
                if (link.profile != null && link.profile != handle?.id) return@LaunchedEffect
                if (signedIn) navController.openPullRequest(link.pr)
            }
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
            profileLabel = label,
            onOpenProfiles = if (hasProfiles) ({ switcherOpen = true }) else null,
            modifier = Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding),
        )
    }
    if (switcherOpen) {
        ProfileSwitcherSheet(
            onDismiss = { switcherOpen = false },
            onPairDesktop = { navController.navigate(Destination.Pair()) },
            onAddTokenProfile = { navController.navigate(Destination.AddTokenProfile) },
        )
    }
}

@Composable
private fun ProfilesUnavailable(state: ProfilesState.Unavailable, container: AppContainer) {
    val colors = RostrumTheme.colors
    val scope = rememberCoroutineScope()
    Box(Modifier.fillMaxSize().background(colors.bg).padding(16.dp), contentAlignment = Alignment.Center) {
        ErrorView(
            state.error,
            title = "Couldn't open this phone's profiles",
            onRetry = { scope.launch { container.profiles.start() } },
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
