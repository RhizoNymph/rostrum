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
import io.github.rhizonymph.rostrum.ui.issue.IssueRoute
import io.github.rhizonymph.rostrum.ui.newissue.NewIssueRoute
import io.github.rhizonymph.rostrum.ui.onboarding.PairRoute
import io.github.rhizonymph.rostrum.ui.repo.RepoRoute
import io.github.rhizonymph.rostrum.ui.onboarding.SignInRoute
import io.github.rhizonymph.rostrum.ui.pr.PullRequestRoute
import io.github.rhizonymph.rostrum.ui.profiles.AddTokenProfileRoute
import io.github.rhizonymph.rostrum.ui.profiles.ProfilesSettingsSection
import io.github.rhizonymph.rostrum.ui.pr.files.FileDiffRoute
import io.github.rhizonymph.rostrum.ui.pr.files.FilesOverviewTab
import io.github.rhizonymph.rostrum.ui.review.SubmitReviewSheet
import io.github.rhizonymph.rostrum.ui.settings.SettingsRoute

/**
 * The navigation graph of one profile (or of first run, [profileLabel]
 * `null`). Features never reference each other: each exposes one `…Route`
 * entry composable taking navigation callbacks, and this file wires them
 * together (including the Files tab and the review sheet into the pull
 * request shell, and the profiles section into Settings). [onOpenProfiles]
 * opens the profile switcher; `null` before any profile exists.
 */
@Composable
fun RostrumNavHost(
    navController: NavHostController,
    signedIn: Boolean,
    profileLabel: String?,
    onOpenProfiles: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val openProfiles = onOpenProfiles ?: {}
    val label = profileLabel.orEmpty()
    NavHost(
        navController = navController,
        startDestination = if (signedIn) Destination.Feed else Destination.SignIn,
        modifier = modifier,
    ) {
        composable<Destination.SignIn> {
            SignInRoute(onPairDesktop = { navController.navigate(Destination.Pair()) }, onOpenProfiles = onOpenProfiles)
        }
        composable<Destination.AddTokenProfile> {
            AddTokenProfileRoute(onBack = { navController.popBackStack() })
        }
        composable<Destination.Pair> { entry ->
            val route = entry.toRoute<Destination.Pair>()
            PairRoute(
                link = route.link,
                onBack = { navController.popBackStack() },
                onPaired = { if (!navController.popBackStack() && signedIn) navController.navigateTopLevel(TopLevel.Feed) },
            )
        }
        composable<Destination.Feed> {
            FeedRoute(
                profileLabel = label,
                onOpenPullRequest = { pr -> navController.openPullRequest(pr) },
                onOpenProfiles = openProfiles,
                onOpenIssue = { issue -> navController.navigate(Destination.Issue(issue.repo, issue.number)) },
                onOpenRepo = { repo -> navController.navigate(Destination.Repo(repo)) },
                onNewIssue = { navController.navigate(Destination.NewIssue()) },
                onPairDesktop = { navController.navigate(Destination.Pair()) },
            )
        }
        composable<Destination.Desktop> {
            DesktopRoute(
                profileLabel = label,
                onOpenPullRequest = { pr, tab -> navController.openPullRequest(pr, tab) },
                onPairDesktop = { navController.navigate(Destination.Pair()) },
                onOpenProfiles = openProfiles,
            )
        }
        composable<Destination.Settings> {
            SettingsRoute(
                onPairDesktop = { navController.navigate(Destination.Pair()) },
                onOpenDesktop = { navController.navigateTopLevel(TopLevel.Desktop) },
                profilesSection = {
                    ProfilesSettingsSection(
                        onPairDesktop = { navController.navigate(Destination.Pair()) },
                        onAddTokenProfile = { navController.navigate(Destination.AddTokenProfile) },
                    )
                },
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
        composable<Destination.Issue> { entry ->
            val route = entry.toRoute<Destination.Issue>()
            IssueRoute(issue = route.issue, onBack = { navController.popBackStack() })
        }
        composable<Destination.NewIssue> { entry ->
            val route = entry.toRoute<Destination.NewIssue>()
            NewIssueRoute(
                repo = route.repo,
                onBack = { navController.popBackStack() },
                onCreated = { issue ->
                    navController.navigate(Destination.Issue(issue.repo, issue.number)) {
                        popUpTo<Destination.NewIssue> { inclusive = true }
                    }
                },
            )
        }
        composable<Destination.Repo> { entry ->
            val route = entry.toRoute<Destination.Repo>()
            RepoRoute(
                repo = route.repo,
                onBack = { navController.popBackStack() },
                onOpenPullRequest = { pr -> navController.openPullRequest(pr) },
                onOpenIssue = { issue -> navController.navigate(Destination.Issue(issue.repo, issue.number)) },
                onNewIssue = { repo -> navController.navigate(Destination.NewIssue(repo)) },
                onPairDesktop = { navController.navigate(Destination.Pair()) },
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
