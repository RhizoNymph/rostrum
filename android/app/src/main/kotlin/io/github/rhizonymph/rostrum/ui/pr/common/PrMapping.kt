package io.github.rhizonymph.rostrum.ui.pr.common

import io.github.rhizonymph.rostrum.data.model.BaseDivergence
import io.github.rhizonymph.rostrum.data.model.CheckRunView
import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.ui.components.CiShape

/**
 * Pure mapping from the backend's pull request records to what the pull
 * request screens show. No Android or Compose, so it is unit-tested.
 */

/** The chip next to the number in the header. */
data class StateBadge(val text: String, val role: ColorRole)

fun stateBadge(state: PullState, isDraft: Boolean): StateBadge = when (state) {
    PullState.Merged -> StateBadge("Merged", ColorRole.Accent)
    PullState.Closed -> StateBadge("Closed", ColorRole.Danger)
    PullState.Open -> if (isDraft) StateBadge("Draft", ColorRole.Draft) else StateBadge("Open", ColorRole.Success)
}

/** The Checks tab's summary card. */
data class ChecksSummary(
    val title: String,
    val subtitle: String,
    val shape: CiShape,
    val role: ColorRole,
)

private data class CheckCounts(val passed: Int, val failing: Int, val running: Int, val skipped: Int)

private fun count(checks: List<CheckRunView>) = CheckCounts(
    passed = checks.count { it.state == CheckState.Success },
    failing = checks.count { it.state == CheckState.Failure || it.state == CheckState.Error },
    running = checks.count { it.state == CheckState.Pending || it.state == CheckState.Expected },
    skipped = checks.count { it.state == null },
)

fun checksSummary(checks: List<CheckRunView>): ChecksSummary {
    if (checks.isEmpty()) {
        return ChecksSummary("No checks", "Nothing reported for this commit", CiShape.None, ColorRole.Neutral)
    }
    val counts = count(checks)
    val subtitle = buildList {
        if (counts.passed > 0) add("${counts.passed} passed")
        if (counts.failing > 0) add("${counts.failing} failing")
        if (counts.running > 0) add("${counts.running} running")
        if (counts.skipped > 0) add("${counts.skipped} skipped")
    }.joinToString(" · ")
    return when {
        counts.failing > 0 -> ChecksSummary("${counts.failing} failing", subtitle, CiShape.Failing, ColorRole.Danger)
        counts.running > 0 -> ChecksSummary("${counts.running} running", subtitle, CiShape.Running, ColorRole.Warning)
        counts.passed > 0 -> ChecksSummary("All checks passed", subtitle, CiShape.Passing, ColorRole.Success)
        else -> ChecksSummary("No results", subtitle, CiShape.None, ColorRole.Neutral)
    }
}

/** A single run's glyph; a run without a state is skipped when it says so. */
fun checkRunShape(run: CheckRunView): CiShape = when (run.state) {
    CheckState.Success -> CiShape.Passing
    CheckState.Failure, CheckState.Error -> CiShape.Failing
    CheckState.Pending, CheckState.Expected -> CiShape.Running
    null -> if (run.statusText.equals("skipped", ignoreCase = true)) CiShape.Skipped else CiShape.None
}

/** One fact tile (Branch tab) or row (merge sheet): label, value, colour. */
data class Fact(val label: String, val value: String, val role: ColorRole, val shape: CiShape)

fun checksFact(checks: List<CheckRunView>): Fact {
    val counts = count(checks)
    return when {
        checks.isEmpty() -> Fact("Checks", "None", ColorRole.Neutral, CiShape.None)
        counts.failing > 0 -> Fact("Checks", "${counts.failing} failing", ColorRole.Danger, CiShape.Failing)
        counts.running > 0 -> Fact("Checks", "${counts.running} running", ColorRole.Warning, CiShape.Running)
        else -> Fact("Checks", "${counts.passed} passing", ColorRole.Success, CiShape.Passing)
    }
}

fun reviewsFact(decision: ReviewDecision?): Fact = when (decision) {
    ReviewDecision.Approved -> Fact("Reviews", "Approved", ColorRole.Success, CiShape.Passing)
    ReviewDecision.ChangesRequested -> Fact("Reviews", "Changes requested", ColorRole.Danger, CiShape.Failing)
    ReviewDecision.ReviewRequired -> Fact("Reviews", "Review required", ColorRole.Warning, CiShape.Running)
    null -> Fact("Reviews", "Not required", ColorRole.Neutral, CiShape.None)
}

fun conflictsFact(status: MergeStatus): Fact = when (status) {
    MergeStatus.Conflicts -> Fact("Conflicts", "Conflicts", ColorRole.Danger, CiShape.Failing)
    MergeStatus.Computing -> Fact("Conflicts", "Checking…", ColorRole.Neutral, CiShape.Running)
    else -> Fact("Conflicts", "None", ColorRole.Success, CiShape.Passing)
}

fun baseFact(divergence: BaseDivergence?): Fact = when {
    divergence == null -> Fact("Base", "Unknown", ColorRole.Neutral, CiShape.None)
    divergence.behind == 0 -> Fact("Base", "Up to date", ColorRole.Success, CiShape.Passing)
    else -> Fact("Base", "↓${divergence.behind} behind ${divergence.baseRef}", ColorRole.Warning, CiShape.Running)
}

fun branchFacts(detail: PullDetail): List<Fact> = listOf(
    checksFact(detail.checks),
    reviewsFact(detail.header.reviewDecision),
    conflictsFact(detail.header.merge.status),
)

fun mergeSheetFacts(detail: PullDetail): List<Fact> = branchFacts(detail) + baseFact(detail.header.divergence)

/** The Branch tab's merge status heading. */
fun mergeStatusTitle(header: PullHeader): String = when (header.state) {
    PullState.Merged -> "Merged"
    PullState.Closed -> "Closed"
    PullState.Open -> when (header.merge.status) {
        MergeStatus.Ready -> "Ready"
        MergeStatus.Unstable -> "Mergeable"
        MergeStatus.Blocked -> "Blocked"
        MergeStatus.Behind -> "Behind"
        MergeStatus.Conflicts -> "Conflicts"
        MergeStatus.Draft -> "Draft"
        MergeStatus.Computing -> "Checking"
    }
}

/** A merge commit's title and message. */
data class CommitText(val title: String, val message: String)

/**
 * GitHub's own defaults, pre-filled so the user sees what will be written.
 * Rebase writes no commit of its own, so it has none.
 */
fun defaultCommit(method: MergeMethod, header: PullHeader): CommitText? = when (method) {
    MergeMethod.Merge -> CommitText(
        "Merge pull request #${header.number} from ${header.repo.substringBefore('/')}/${header.headRef}",
        header.title,
    )
    MergeMethod.Squash -> CommitText("${header.title} (#${header.number})", "")
    MergeMethod.Rebase -> null
}

fun methodLabel(method: MergeMethod): String = when (method) {
    MergeMethod.Merge -> "Merge commit"
    MergeMethod.Squash -> "Squash"
    MergeMethod.Rebase -> "Rebase"
}

/** "Update adds main's 4 new commits to feat/diff-overview on GitHub." */
fun baseUpdateExplanation(divergence: BaseDivergence, headRef: String): String {
    val commits = if (divergence.behind == 1) "1 new commit" else "${divergence.behind} new commits"
    return "Update adds ${divergence.baseRef}'s $commits to $headRef on GitHub."
}
