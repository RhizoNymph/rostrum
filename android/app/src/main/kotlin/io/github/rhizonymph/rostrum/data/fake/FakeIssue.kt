package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.IssueCloseReason
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.IssueStatus
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Instant

/** An issue as the fake stores it. */
internal data class FakeIssue(
    val repo: String,
    val number: Int,
    val title: String,
    val body: String,
    val author: String,
    val createdAt: Instant,
    val updatedAt: Instant,
    val status: IssueStatus = IssueStatus.Open,
    val labels: List<String> = emptyList(),
    val assignees: List<String> = emptyList(),
    val comments: Int = 0,
    val milestone: String? = null,
) {
    val ref: IssueRef get() = IssueRef(repo, number)

    val isOpen: Boolean get() = status == IssueStatus.Open

    /** Lowercase logins this issue involves besides its author (for "Involved"). */
    val involved: Set<String> get() = assignees.mapTo(mutableSetOf()) { it.lowercase() }

    fun summary(viewer: String?, labelsByName: Map<String, LabelView>): IssueSummary = IssueSummary(
        repo = repo,
        number = number,
        title = title,
        url = "https://github.com/$repo/issues/$number",
        status = status,
        statusChip = statusChip(status),
        author = UserRef(author),
        createdAt = createdAt,
        updatedAt = updatedAt,
        labels = labels.map { labelsByName[it] ?: LabelView(it, null) },
        assignees = assignees.map { UserRef(it) },
        commentCount = comments,
        milestone = milestone,
        isYours = viewer != null && author.equals(viewer, ignoreCase = true),
        assignedToYou = viewer != null && assignees.any { it.equals(viewer, ignoreCase = true) },
    )

    companion object {
        fun statusChip(status: IssueStatus): Chip = when (status) {
            IssueStatus.Open -> Chip("open", ColorRole.Success)
            is IssueStatus.Closed -> when (status.reason) {
                IssueCloseReason.NotPlanned -> Chip("not planned", ColorRole.Neutral)
                IssueCloseReason.Duplicate -> Chip("duplicate", ColorRole.Neutral)
                IssueCloseReason.Completed, null -> Chip("completed", ColorRole.Accent)
            }
        }
    }
}

/** The mockups' issues: some in rostrum and zed, none elsewhere. */
internal object SampleIssues {
    fun issues(now: Instant): List<FakeIssue> {
        fun ago(minutes: Long): Instant = now.minusSeconds(minutes * 60)
        val hour = 60L
        val day = 24 * hour
        return listOf(
            FakeIssue(
                repo = SamplePulls.ROSTRUM, number = 21,
                title = "Feed forgets the scroll position after a background refresh",
                body = "After the 60 s refresh the list jumps to the top.\n\n- open the feed\n- scroll down\n- wait a minute",
                author = "ada-lin", createdAt = ago(3 * hour), updatedAt = ago(hour),
                labels = listOf("bug", "ui"), assignees = listOf(SamplePulls.VIEWER), comments = 2,
            ),
            FakeIssue(
                repo = SamplePulls.ROSTRUM, number = 18,
                title = "Show issues on Android",
                body = "The phone should list issues beside pull requests.",
                author = SamplePulls.VIEWER, createdAt = ago(2 * day), updatedAt = ago(day),
                labels = listOf("android", "enhancement"), milestone = "0.2",
            ),
            FakeIssue(
                repo = SamplePulls.ZED, number = 20410,
                title = "Git panel freezes on a repository with 40k untracked files",
                body = "Reproducible on Linux with a large monorepo.",
                author = "tjvance", createdAt = ago(day), updatedAt = ago(5 * hour),
                labels = listOf("area:git", "performance"), assignees = listOf("ada-lin"), comments = 5,
            ),
            FakeIssue(
                repo = SamplePulls.ZED, number = 20388,
                title = "vim: `gv` doesn't restore a visual block",
                body = "Expected the last block selection.",
                author = "wren", createdAt = ago(4 * day), updatedAt = ago(3 * day),
                labels = listOf("vim"),
            ),
        )
    }

    /** Who the fake lets issues be assigned to. */
    fun assignable(repo: String): List<UserRef> = when (repo) {
        SamplePulls.ROSTRUM -> listOf(SamplePulls.VIEWER, "ada-lin", "mkowal")
        SamplePulls.ZED -> listOf("ada-lin", "tjvance", "mkowal", "wren")
        else -> listOf(SamplePulls.VIEWER)
    }.map { UserRef(it) }
}
