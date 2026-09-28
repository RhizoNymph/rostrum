package io.github.rhizonymph.rostrum.ui.navigation

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.toRoute
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.desktop.DesktopRoute
import io.github.rhizonymph.rostrum.ui.feed.FeedRoute
import io.github.rhizonymph.rostrum.ui.onboarding.PairRoute
import io.github.rhizonymph.rostrum.ui.onboarding.SignInRoute
import io.github.rhizonymph.rostrum.ui.pr.PullRequestRoute
import io.github.rhizonymph.rostrum.ui.pr.files.FileDiffRoute
import io.github.rhizonymph.rostrum.ui.pr.files.FilesOverviewTab
import io.github.rhizonymph.rostrum.ui.review.SubmitReviewSheet
import io.github.rhizonymph.rostrum.ui.settings.SettingsRoute

/**
 * The navigation graph. Features never reference each other: each exposes one
 * `…Route` entry composable taking navigation callbacks, and this file wires
 * them together (including the Files tab and the review sheet into the pull
 * request shell).
 */
@Composable
fun RostrumNavHost(
    navController: NavHostController,
    signedIn: Boolean,
    modifier: Modifier = Modifier,
) {
    NavHost(
        navController = navController,
        startDestination = if (signedIn) Destination.Feed else Destination.SignIn,
        modifier = modifier,
    ) {
        composable<Destination.SignIn> {
            SignInRoute(onPairDesktop = { navController.navigate(Destination.Pair()) })
        }
        composable<Destination.Pair> { entry ->
            val route = entry.toRoute<Destination.Pair>()
            PairRoute(
                link = route.link,
                onBack = { navController.popBackStack() },
                onPaired = { if (!navController.popBackStack()) navController.navigateTopLevel(TopLevel.Feed) },
            )
        }
        composable<Destination.Feed> {
            FeedRoute(
                onOpenPullRequest = { pr -> navController.openPullRequest(pr) },
                onOpenDesktop = { navController.navigateTopLevel(TopLevel.Desktop) },
            )
        }
        composable<Destination.Desktop> {
            DesktopRoute(
                onOpenPullRequest = { pr, tab -> navController.openPullRequest(pr, tab) },
                onPairDesktop = { navController.navigate(Destination.Pair()) },
            )
        }
        composable<Destination.Settings> {
            SettingsRoute(
                onPairDesktop = { navController.navigate(Destination.Pair()) },
                onOpenDesktop = { navController.navigateTopLevel(TopLevel.Desktop) },
            )
        }
        composable<Destination.PullRequest> { entry ->
            val route = entry.toRoute<Destination.PullRequest>()
            val pr = route.pr
            PullRequestRoute(
                pr = pr,
                initialTab = route.tab,
                onBack = { navController.popBackStack() },
                filesTab = { tabModifier ->
                    FilesOverviewTab(
                        pr = pr,
                        onOpenFile = { index -> navController.navigate(Destination.FileDiff(pr.repo, pr.number, index)) },
                        modifier = tabModifier,
                    )
                },
                reviewSheet = { onDismiss, onSubmitted -> SubmitReviewSheet(pr, onDismiss, onSubmitted) },
            )
        }
        composable<Destination.FileDiff> { entry ->
            val route = entry.toRoute<Destination.FileDiff>()
            FileDiffRoute(pr = route.pr, fileIndex = route.fileIndex, onBack = { navController.popBackStack() })
        }
    }
}

fun NavHostController.openPullRequest(pr: PrRef, tab: PrTab = PrTab.Conversation) {
    navigate(Destination.PullRequest(pr.repo, pr.number, tab))
}

/** Switch bottom-bar tab, keeping each tab's own back stack and state. */
fun NavHostController.navigateTopLevel(item: TopLevel) {
    navigate(item.destination) {
        popUpTo(graph.findStartDestination().id) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
}
