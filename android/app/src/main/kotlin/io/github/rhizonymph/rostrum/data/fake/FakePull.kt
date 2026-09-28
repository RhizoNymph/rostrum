package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.BaseDivergence
import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.DraftAction
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.MergeVerdict
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Instant

/** A pull request as the fake stores it; the records the UI sees derive from it. */
internal data class FakePull(
    val repo: String,
    val number: Int,
    val title: String,
    val author: String,
    val createdAt: Instant,
    val updatedAt: Instant,
    val isDraft: Boolean = false,
    val state: PullState = PullState.Open,
    val checks: CheckState?,
    val reviewDecision: ReviewDecision? = ReviewDecision.ReviewRequired,
    val mergeStatus: MergeStatus,
    /** Commits on the base this branch lacks; `null` before the compare answers. */
    val behind: Int?,
    val ahead: Int = 1,
    val labels: List<String> = emptyList(),
    val additions: Int,
    val deletions: Int,
    val changedFiles: Int,
    val comments: Int = 0,
    /** Logins whose review is outstanding. */
    val reviewers: List<String> = emptyList(),
    val assignees: List<String> = emptyList(),
    val headRef: String,
    val baseRef: String = "main",
    val headSha: String,
    /** Chips the paired desktop contributes: `handed off`, `↑2 unpushed`. */
    val localChips: List<Chip> = emptyList(),
) {
    val ref: PrRef get() = PrRef(repo, number)

    /** Lowercase logins this pull request involves besides its author. */
    val involved: Set<String> get() = (reviewers + assignees).mapTo(mutableSetOf()) { it.lowercase() }

    fun reviewRequestedFrom(viewer: String?): Boolean =
        viewer != null && reviewers.any { it.equals(viewer, ignoreCase = true) }

    fun divergence(): BaseDivergence? = behind?.let {
        BaseDivergence(
            behind = it,
            ahead = ahead,
            baseRef = baseRef,
            fastForwards = ahead == 0,
            summary = "$it commit${if (it == 1) "" else "s"} behind $baseRef, $ahead ahead",
        )
    }

    fun summary(viewer: String?, labelsByName: Map<String, LabelView>): PrSummary = PrSummary(
        repo = repo,
        number = number,
        title = title,
        url = "https://github.com/$repo/pull/$number",
        author = UserRef(author),
        createdAt = createdAt,
        updatedAt = updatedAt,
        isDraft = isDraft,
        checks = checks,
        checksRole = checksRole(checks),
        reviewDecision = reviewDecision,
        reviewChip = reviewChip(reviewDecision),
        mergeStatus = mergeStatus,
        mergeChip = if (mergeStatus == MergeStatus.Conflicts) Chip("conflict", ColorRole.Danger, "Conflicts with $baseRef") else null,
        baseDivergence = divergence(),
        behindChip = behind?.takeIf { it > 0 }?.let {
            Chip("↓$it $baseRef", ColorRole.Warning, "$it commit${if (it == 1) "" else "s"} behind $baseRef")
        },
        labels = labels.map { labelsByName[it] ?: LabelView(it, null) },
        additions = additions,
        deletions = deletions,
        changedFiles = changedFiles,
        commentCount = comments,
        reviewRequested = reviewRequestedFrom(viewer),
        isYours = viewer != null && author.equals(viewer, ignoreCase = true),
        headRef = headRef,
        baseRef = baseRef,
        localChips = localChips,
    )

    fun header(viewer: String?, labelsByName: Map<String, LabelView>): PullHeader = PullHeader(
        repo = repo,
        number = number,
        title = title,
        url = "https://github.com/$repo/pull/$number",
        state = state,
        isDraft = isDraft,
        author = UserRef(author),
        createdAt = createdAt,
        updatedAt = updatedAt,
        headRef = headRef,
        baseRef = baseRef,
        headSha = headSha,
        labels = labels.map { labelsByName[it] ?: LabelView(it, null) },
        assignees = assignees.map { UserRef(it) },
        reviewRequests = reviewers.map { UserRef(it) },
        reviewDecision = reviewDecision,
        reviewChip = reviewChip(reviewDecision),
        merge = verdict(),
        divergence = divergence(),
        checks = checks,
        checksRole = checksRole(checks),
        changedFiles = changedFiles,
        additions = additions,
        deletions = deletions,
        commentCount = comments,
        isYours = viewer != null && author.equals(viewer, ignoreCase = true),
        reviewRequested = reviewRequestedFrom(viewer),
        draftAction = if (isDraft) DraftAction(false, "Ready for review") else DraftAction(true, "Convert to draft"),
    )

    fun verdict(): MergeVerdict = when {
        state == PullState.Merged -> MergeVerdict(mergeStatus, "Merged", true, ColorRole.Accent, null)
        state == PullState.Closed -> MergeVerdict(mergeStatus, "Closed without merging", true, ColorRole.Danger, null)
        else -> when (mergeStatus) {
            MergeStatus.Computing -> MergeVerdict(mergeStatus, "Checking mergeability…", true, ColorRole.Neutral, null)
            MergeStatus.Conflicts -> MergeVerdict(
                mergeStatus, "Conflicts with $baseRef · resolve before merging", true, ColorRole.Danger,
                Chip("conflict", ColorRole.Danger),
            )
            MergeStatus.Draft -> MergeVerdict(mergeStatus, "Draft · mark ready for review to merge", true, ColorRole.Draft, Chip("draft", ColorRole.Draft))
            MergeStatus.Blocked -> MergeVerdict(
                mergeStatus, "Blocked · 1 approving review required", true, ColorRole.Warning,
                Chip("blocked", ColorRole.Warning),
            )
            MergeStatus.Behind -> MergeVerdict(
                mergeStatus, "Behind $baseRef · update the branch to merge", true, ColorRole.Warning,
                Chip("behind", ColorRole.Warning),
            )
            MergeStatus.Unstable -> MergeVerdict(mergeStatus, "Mergeable · some checks are not passing", false, ColorRole.Warning, null)
            MergeStatus.Ready -> MergeVerdict(mergeStatus, "Ready · approved, checks passing", false, ColorRole.Success, null)
        }
    }

    companion object {
        fun checksRole(checks: CheckState?): ColorRole = when (checks) {
            CheckState.Success -> ColorRole.Success
            CheckState.Failure, CheckState.Error -> ColorRole.Danger
            CheckState.Pending, CheckState.Expected -> ColorRole.Warning
            null -> ColorRole.Neutral
        }

        fun reviewChip(decision: ReviewDecision?): Chip? = when (decision) {
            ReviewDecision.Approved -> Chip("Approved", ColorRole.Success)
            ReviewDecision.ChangesRequested -> Chip("Changes requested", ColorRole.Danger)
            ReviewDecision.ReviewRequired, null -> null
        }
    }
}
