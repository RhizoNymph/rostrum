package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.PrRef
import kotlinx.serialization.Serializable

/** Every screen the app navigates to, as a typed Navigation Compose route. */
@Serializable
sealed interface Destination {
    /** First run: pair with the desktop, or paste a token. */
    @Serializable
    data object SignIn : Destination

    /** Pairing. [link] is a `rostrum://pair?…` deep link to preview; `null` starts on the instructions. */
    @Serializable
    data class Pair(val link: String? = null) : Destination

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
