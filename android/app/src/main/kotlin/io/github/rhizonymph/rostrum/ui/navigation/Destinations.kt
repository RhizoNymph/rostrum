package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import kotlinx.serialization.Serializable

/** Every screen the app navigates to, as a typed Navigation Compose route. */
@Serializable
sealed interface Destination {
    /** First run, or the active profile lost its token: pair with a desktop, or paste a token. */
    @Serializable
    data object SignIn : Destination

    /** Pairing. [link] is a `rostrum://pair?…` deep link to preview; `null` starts on the instructions. */
    @Serializable
    data class Pair(val link: String? = null) : Destination

    /** A new profile for a pasted GitHub token (from the profile switcher or Settings). */
    @Serializable
    data object AddTokenProfile : Destination

    @Serializable
    data object Feed : Destination

    @Serializable
    data object Desktop : Destination

    @Serializable
    data object Settings : Destination

    @Serializable
    data class PullRequest(val repo: String, val number: Int, val tab: PrTab = PrTab.Conversation) : Destination {
        val pr: PrRef get() = PrRef(repo, number)
    }

    /** One issue: header, labels, assignees, timeline, composer. */
    @Serializable
    data class Issue(val repo: String, val number: Int) : Destination {
        val issue: IssueRef get() = IssueRef(repo, number)
    }

    /** The new-issue form, preset to [repo] when opened from a repository's screen. */
    @Serializable
    data class NewIssue(val repo: String? = null) : Destination

    /** One repository's screen: its pull requests, issues and branches. */
    @Serializable
    data class Repo(val repo: String) : Destination

    /** The CI grid: every repository's checks, or [repo]'s alone. */
    @Serializable
    data class Checks(val repo: String? = null) : Destination

    /** One file's diff; previous/next move within the screen. */
    @Serializable
    data class FileDiff(val repo: String, val number: Int, val fileIndex: Int) : Destination {
        val pr: PrRef get() = PrRef(repo, number)
    }
}

/** The pull request screen's four tabs. */
@Serializable
enum class PrTab(val title: String) {
    Conversation("Conversation"),
    Files("Files"),
    Checks("Checks"),
    Branch("Branch"),
}

/** The three bottom-bar destinations. */
enum class TopLevel(val destination: Destination, val label: String) {
    Feed(Destination.Feed, "Feed"),
    Desktop(Destination.Desktop, "Desktop"),
    Settings(Destination.Settings, "Settings"),
}
