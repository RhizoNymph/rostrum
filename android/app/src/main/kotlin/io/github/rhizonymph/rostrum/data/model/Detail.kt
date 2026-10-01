package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/** Everything the Conversation, Checks and Branch tabs show (`detail/types.rs`). */
data class PullDetail(
    val header: PullHeader,
    /** Description first, then comments, reviews and events, oldest first. */
    val timeline: List<TimelineEntry>,
    /** Inline review threads; reviews reference them by id. */
    val threads: List<ReviewThreadView>,
    val checks: List<CheckRunView>,
    val unresolvedThreads: Int,
    /** Your unsent inline comments on this pull request. */
    val pendingReview: PendingReview,
)

/** The header, and the facts the Branch tab and the action bar decide on. */
data class PullHeader(
    val repo: String,
    val number: Int,
    val title: String,
    val url: String,
    val state: PullState,
    val isDraft: Boolean,
    val author: UserRef?,
    val createdAt: Instant,
    val updatedAt: Instant,
    val headRef: String,
    val baseRef: String,
    /** Pass back as the expected head to merge and updateBranch. */
    val headSha: String,
    val labels: List<LabelView>,
    val assignees: List<UserRef>,
    /** People (not teams) whose review is outstanding. */
    val reviewRequests: List<UserRef>,
    val reviewDecision: ReviewDecision?,
    val reviewChip: Chip?,
    val merge: MergeVerdict,
    val divergence: BaseDivergence?,
    val checks: CheckState?,
    val checksRole: ColorRole,
    val changedFiles: Int,
    val additions: Int,
    val deletions: Int,
    val commentCount: Int,
    val isYours: Boolean,
    val reviewRequested: Boolean,
    /** The draft toggle, fixed now so a refresh can never invert it. */
    val draftAction: DraftAction,
) {
    val ref: PrRef get() = PrRef(repo, number)
}

/** Can this be merged, and if not, why. */
data class MergeVerdict(
    val status: MergeStatus,
    /** "Blocked · 1 approving review required". */
    val sentence: String,
    /** Disable the merge button. */
    val blocksMerge: Boolean,
    /** Colour of the verdict's dot. */
    val role: ColorRole,
    val chip: Chip?,
)

/** What the draft toggle does when tapped. */
data class DraftAction(
    /** Pass to setDraft: `true` converts to draft, `false` marks ready. */
    val toDraft: Boolean,
    /** "Convert to draft" or "Ready for review". */
    val label: String,
)

/** One item of the conversation. */
data class TimelineEntry(
    /** Stable key for list diffing. */
    val id: String,
    val author: UserRef?,
    val createdAt: Instant,
    val kind: TimelineKind,
)

sealed interface TimelineKind {
    /** The pull request's description; always first. */
    data class Description(val body: List<MdBlock>, val source: String) : TimelineKind

    data class Comment(val body: List<MdBlock>, val source: String) : TimelineKind

    data class Review(
        val state: ReviewState,
        val chip: Chip,
        val body: List<MdBlock>,
        val source: String,
        /** Threads this review opened, by [ReviewThreadView.id]. */
        val threadIds: List<String>,
    ) : TimelineKind

    data class Event(
        val event: TimelineEvent,
        /** "pushed 2 commits", to follow the actor's login. */
        val text: String,
    ) : TimelineKind
}

sealed interface TimelineEvent {
    data object Merged : TimelineEvent
    data object Closed : TimelineEvent

    /** An issue closed with a reason. */
    data class ClosedAs(val reason: IssueCloseReason) : TimelineEvent
    data object Reopened : TimelineEvent
    data object ReadyForReview : TimelineEvent
    data object ConvertedToDraft : TimelineEvent
    data object ForcePushed : TimelineEvent
    data class ReviewRequested(val reviewer: String) : TimelineEvent
    data class Assigned(val assignee: String) : TimelineEvent
    data class Unassigned(val assignee: String) : TimelineEvent

    /** Mentioned from another issue or pull request ([source] is `owner/name#N`). */
    data class CrossReferenced(val source: String, val title: String) : TimelineEvent
    data class Labeled(val label: String) : TimelineEvent
    data class Unlabeled(val label: String) : TimelineEvent
    data class Renamed(val from: String, val to: String) : TimelineEvent
    data class Other(val kind: String) : TimelineEvent
}

/** An inline review thread. */
data class ReviewThreadView(
    val id: String,
    val path: String,
    /** Line in the current diff; `null` once outdated. */
    val line: Int?,
    val originalLine: Int?,
    val side: Side,
    val resolved: Boolean,
    val outdated: Boolean,
    /** `src/main.rs:12`, or `src/main.rs (outdated)`. */
    val location: String,
    val comments: List<ThreadCommentView>,
    /** Whether replyToThread can reply here. */
    val canReply: Boolean,
)

data class ThreadCommentView(
    val id: String,
    val author: UserRef?,
    val createdAt: Instant,
    val body: List<MdBlock>,
    val source: String,
)

/** One CI check on the head commit. */
data class CheckRunView(
    val name: String,
    val state: CheckState?,
    val role: ColorRole,
    /** `success`, `failure`, `4m 12s`, … or `no status`. */
    val statusText: String,
    val url: String?,
)

/** How GitHub should merge. */
enum class MergeMethod { Merge, Squash, Rebase }

/** How to bring a branch up to date with its base, on GitHub's side. */
enum class BranchUpdateMethod { Merge, Rebase }
