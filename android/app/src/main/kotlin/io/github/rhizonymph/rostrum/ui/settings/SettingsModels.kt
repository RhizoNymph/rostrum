package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.ui.common.UiState

/** Who the GitHub token belongs to, if GitHub said. */
sealed interface AccountViewer {
    data class Known(val login: String) : AccountViewer

    /** The lookup failed (offline, rate limited); the token is still in use. */
    data class Unknown(val error: BackendError) : AccountViewer
}

data class AccountInfo(val viewer: AccountViewer, val host: String)

/** The second line of a repository row. */
sealed interface RepoDetail {
    /** The paired desktop has a clone at [path]. */
    data class Clone(val machine: String, val path: String) : RepoDetail

    /** Paired, but the desktop has no clone of it. */
    data object NoClone : RepoDetail

    /** Loaded with nothing to show, so the feed hides it. */
    data object HiddenEmpty : RepoDetail

    /** Nothing worth saying (no desktop to ask). */
    data object None : RepoDetail
}

data class RepoRow(val repo: String, val detail: RepoDetail)

/** The Settings screen's desktop row. */
sealed interface DesktopSummary {
    data object NotPaired : DesktopSummary

    data class Connected(val machine: MachineInfo) : DesktopSummary

    data class Unreachable(val error: BackendError) : DesktopSummary
}

data class SettingsContent(
    val account: AccountInfo,
    val repos: List<RepoRow>,
    val desktop: DesktopSummary,
    val refreshIntervalSecs: Long,
    val notifyNewPullRequests: Boolean,
    val notifyReviewRequests: Boolean,
)

/** The "Add a repository" field. [error] is only ever an input problem. */
data class AddRepoState(
    val input: String = "",
    val error: BackendError? = null,
    val running: Boolean = false,
)

data class SettingsUiState(
    val content: UiState<SettingsContent> = UiState.Loading,
    val addRepo: AddRepoState = AddRepoState(),
    /** Repositories whose removal is in flight. */
    val removing: Set<String> = emptySet(),
)

/** The foreground refresh choices offered, in seconds. */
val RefreshIntervalChoices: List<Long> = listOf(30, 60, 300, 900)

/**
 * Each watched repository with what the phone knows about it: a clone on the
 * desktop, no clone, or hidden from the feed for having nothing open.
 */
fun repoRows(repos: List<String>, feed: FeedSnapshot?, machine: MachineInfo?): List<RepoRow> {
    val shown = feed?.repos?.mapTo(mutableSetOf()) { it.repo.lowercase() }
    val hideEmpty = feed?.preferences?.hideEmptyRepos == true
    return repos.map { repo ->
        val clone = machine?.clones?.firstOrNull { it.repo.equals(repo, ignoreCase = true) }
        val detail = when {
            hideEmpty && shown != null && repo.lowercase() !in shown -> RepoDetail.HiddenEmpty
            machine == null -> RepoDetail.None
            clone != null -> RepoDetail.Clone(machine.name, clone.path)
            else -> RepoDetail.NoClone
        }
        RepoRow(repo, detail)
    }
}

fun RepoDetail.subline(): String? = when (this) {
    is RepoDetail.Clone -> "clone on $machine · $path"
    RepoDetail.NoClone -> "no clone"
    RepoDetail.HiddenEmpty -> "no open pull requests · hidden"
    RepoDetail.None -> null
}

/** `every 60 s`, `every 5 min`. */
fun refreshIntervalLabel(seconds: Long): String =
    if (seconds > 60 && seconds % 60 == 0L) "every ${seconds / 60} min" else "every $seconds s"

/** An error line whose [code] part (a repository name) is set in mono. */
data class InlineError(val code: String?, val rest: String)

fun addRepoError(error: BackendError): InlineError = when (error) {
    is BackendError.DuplicateRepo -> InlineError(error.repo, " is already in your feed")
    is BackendError.InvalidRepo -> InlineError(error.input, " isn't a repository: ${error.reason}")
    else -> InlineError(null, error.describe())
}

/** Whether an add failure belongs under the field (input problems) or in the snackbar. */
val BackendError.isRepoInputError: Boolean
    get() = this is BackendError.DuplicateRepo || this is BackendError.InvalidRepo

fun accountSubline(host: String): String = "$host · signed in with a token"
