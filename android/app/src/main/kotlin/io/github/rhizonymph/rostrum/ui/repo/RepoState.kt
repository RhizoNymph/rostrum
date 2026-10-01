package io.github.rhizonymph.rostrum.ui.repo

import io.github.rhizonymph.rostrum.data.model.BranchDrift
import io.github.rhizonymph.rostrum.data.model.BranchNote
import io.github.rhizonymph.rostrum.data.model.BranchTree
import io.github.rhizonymph.rostrum.data.model.RepoOverview
import io.github.rhizonymph.rostrum.data.model.TrunkDrift
import io.github.rhizonymph.rostrum.data.model.TrunkSettings
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import java.time.Instant

/** The repository screen's three tabs. */
enum class RepoTab(val title: String) { Pulls("Pull requests"), Issues("Issues"), Branches("Branches") }

/** The trunk editor: detection, or a list typed by hand. */
data class TrunkEditor(val detect: Boolean, val text: String, val save: ActionState = ActionState.Idle) {
    companion object {
        fun of(settings: TrunkSettings) = TrunkEditor(detect = settings.detected, text = settings.configured.joinToString(", "))
    }
}

data class RepoUiState(
    val tab: RepoTab = RepoTab.Pulls,
    val overview: UiState<RepoOverview> = UiState.Loading,
    /** `null` until the Branches tab is first shown. */
    val branches: UiState<BranchTree>? = null,
    val trunkEditor: TrunkEditor? = null,
    val refreshing: Boolean = false,
    val now: Instant,
)

/** `main, develop` or `main develop` → the names, blanks and repeats dropped. */
fun parseTrunkNames(text: String): List<String> =
    text.split(',', ' ', '\n', '\t').map { it.trim() }.filter { it.isNotEmpty() }.distinct()

/** `↑2 ↓5` (commits ahead and behind), `even` when neither. */
fun driftText(drift: BranchDrift): String = when {
    drift.ahead == 0 && drift.behind == 0 -> "even"
    else -> listOfNotNull(drift.ahead.takeIf { it > 0 }?.let { "↑$it" }, drift.behind.takeIf { it > 0 }?.let { "↓$it" }).joinToString(" ")
}

/** A trunk against the default branch: `default`, `missing`, `↑2 ↓5 vs main`. */
fun trunkDriftText(drift: TrunkDrift, defaultBranch: String?): String = when (drift) {
    TrunkDrift.Default -> "default branch"
    TrunkDrift.Missing -> "no such branch"
    TrunkDrift.Unknown -> "couldn't compare"
    is TrunkDrift.Known -> "${driftText(drift.drift)} vs ${defaultBranch ?: "default"}"
}

fun noteText(note: BranchNote): String = when (note) {
    BranchNote.BreaksCycle -> "its base chain loops; shown here to break it"
    BranchNote.AmbiguousBase -> "more than one pull request has this base as head"
}

/** What the trunk line under the Branches tab says. */
fun trunksSummary(settings: TrunkSettings): String = when {
    settings.detected -> "Trunks: detected (${settings.existing.joinToString(", ").ifEmpty { "none found" }})"
    else -> "Trunks: ${settings.configured.joinToString(", ")}"
}
