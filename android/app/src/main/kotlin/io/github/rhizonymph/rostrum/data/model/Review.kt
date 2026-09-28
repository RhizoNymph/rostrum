package io.github.rhizonymph.rostrum.data.model

/** Your unsent inline comments on one pull request (`review/types.rs`). */
data class PendingReview(
    val repo: String,
    val number: Int,
    val drafts: List<ReviewDraft>,
    /** The head commit the drafts were anchored against; `null` when none. */
    val draftedAgainst: String?,
    /** The pull request's current head commit. */
    val headSha: String,
    /**
     * The head moved after the drafts were written. Stale drafts cannot be
     * submitted or added to; discard them and re-read the diff.
     */
    val stale: Boolean,
) {
    val isEmpty: Boolean get() = drafts.isEmpty()
}

/** One pending inline comment. */
data class ReviewDraft(
    /** Stable for the life of the process; pass to editDraft and removeDraft. */
    val id: Long,
    val anchor: DraftAnchor,
    val body: String,
    /** `src/main.rs:12` or `src/main.rs lines 12–18`. */
    val location: String,
)

/** Where a draft attaches. A range spans `startLine..=line` on one side. */
data class DraftAnchor(
    val path: String,
    val line: Int,
    val side: Side,
    /** First line of a multi-line comment; `null` for a single line. */
    val startLine: Int?,
)

/** The verdict a review is submitted with. */
enum class ReviewEvent { Comment, Approve, RequestChanges }
