package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/** Why an issue was closed. Duplicate is shown, never chosen here. */
enum class IssueCloseReason { Completed, NotPlanned, Duplicate }

/** The reasons the app can close an issue with. */
enum class CloseIssueAs { Completed, NotPlanned }

/** An issue's state; a close reason only exists on a closed issue. */
sealed interface IssueStatus {
    data object Open : IssueStatus

    data class Closed(val reason: IssueCloseReason?) : IssueStatus
}

/** An issue as a row and as the issue screen's header. */
data class IssueSummary(
    val repo: String,
    val number: Int,
    val title: String,
    val url: String,
    val status: IssueStatus,
    /** `open`, `completed`, `not planned`, `duplicate`. */
    val statusChip: Chip,
    val author: UserRef?,
    val createdAt: Instant,
    val updatedAt: Instant,
    val labels: List<LabelView>,
    val assignees: List<UserRef>,
    val commentCount: Int,
    val milestone: String?,
    val isYours: Boolean,
    val assignedToYou: Boolean,
) {
    val ref: IssueRef get() = IssueRef(repo, number)
}

/** One issue. Distinct from [PrRef]: which screen opens depends on the kind. */
data class IssueRef(val repo: String, val number: Int) {
    init {
        require(number > 0) { "issue numbers start at 1" }
    }

    override fun toString(): String = "$repo#$number"
}

/** The issue screen: its header and its timeline (body, comments, events). */
data class IssueDetail(val issue: IssueSummary, val timeline: List<TimelineEntry>)
