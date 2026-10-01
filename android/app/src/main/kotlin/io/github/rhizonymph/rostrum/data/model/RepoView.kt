package io.github.rhizonymph.rostrum.data.model

/** One repository's screen: every open pull request and issue, unfiltered, in the item sort. */
data class RepoOverview(
    val repo: String,
    val url: String,
    val stars: Int?,
    val defaultBranch: String?,
    val pulls: List<PullItem>,
    val issues: List<IssueSummary>,
    val pullsLoad: RepoLoad,
    val issuesLoad: RepoLoad,
)

/** How far a branch is from another: commits only it has, and commits it lacks. */
data class BranchDrift(val ahead: Int, val behind: Int)

/** A trunk's distance from the default branch. */
sealed interface TrunkDrift {
    /** It is the default branch. */
    data object Default : TrunkDrift

    /** Configured, but no such branch exists. */
    data object Missing : TrunkDrift

    /** The comparison failed. */
    data object Unknown : TrunkDrift

    data class Known(val drift: BranchDrift) : TrunkDrift
}

/** Why a pull request row sits where it does. */
enum class BranchNote { BreaksCycle, AmbiguousBase }

/** One row of the branch tree, in display order. */
sealed interface BranchRow {
    /** A trunk with its open pull requests beneath. */
    data class Trunk(val name: String, val drift: TrunkDrift, val pulls: Int) : BranchRow

    /** The divider before bases that are no trunk. */
    data object OtherBases : BranchRow

    /** A base branch that is not a trunk. */
    data class Base(val name: String, val pulls: Int) : BranchRow

    /** A pull request, nested [depth] levels under its trunk or base. */
    data class Pull(
        val depth: Int,
        val number: Int,
        val head: String,
        val base: String,
        val drift: BranchDrift?,
        val note: BranchNote?,
        /** `stack 7` or `chain`. */
        val stackLabel: String?,
        val pull: PrSummary?,
    ) : BranchRow
}

/** Which branches count as trunks: detected, or a configured list. */
data class TrunkSettings(
    val detected: Boolean,
    val configured: List<String>,
    /** The trunk names that exist in the repository. */
    val existing: List<String>,
)

/** The repository's branches as a tree under its trunks. */
data class BranchTree(
    val repo: String,
    val url: String,
    val stars: Int,
    val defaultBranch: String?,
    val trunks: TrunkSettings,
    val rows: List<BranchRow>,
)
