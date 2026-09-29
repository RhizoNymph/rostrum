package io.github.rhizonymph.rostrum.ui.review

import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.ui.format.shortSha

/** What a review may be submitted as, decided in one place so the sheet and the button agree. */
object ReviewRules {
    /**
     * Why [event] is unavailable, or `null` when it is allowed. Commenting is
     * always allowed. Approving or requesting changes is not on your own pull
     * request (GitHub refuses), nor while your drafts are stale: new commits
     * arrived that you have not looked at.
     */
    fun blockedReason(event: ReviewEvent, header: PullHeader, pending: PendingReview): String? = when {
        event == ReviewEvent.Comment -> null
        header.isYours -> "You can't approve or request changes on your own pull request."
        pending.stale -> "New commits arrived after you drafted. Discard the drafts and re-check the diff first."
        else -> null
    }

    /** Stale drafts cannot be sent (their lines may have moved), so they only go with a current review. */
    fun includesDrafts(pending: PendingReview): Boolean = pending.drafts.isNotEmpty() && !pending.stale

    /** An approval may be empty; a comment or change request needs a summary or drafts to send. */
    fun hasContent(event: ReviewEvent, summary: String, pending: PendingReview): Boolean =
        event == ReviewEvent.Approve || summary.isNotBlank() || includesDrafts(pending)

    /** The warning shown over stale drafts, or `null` when they are current. */
    fun staleWarning(header: PullHeader, pending: PendingReview): String? {
        if (!pending.stale) return null
        val author = header.author?.login ?: "The author"
        val moved = pending.draftedAgainst?.let { "${shortSha(it)} → ${shortSha(pending.headSha)}" } ?: shortSha(pending.headSha)
        return "$author pushed after you drafted these comments ($moved). Their lines may have moved, so they " +
            "can't be sent: discard them and re-check the diff. Approve and Request changes are off until then."
    }

    /** The subtitle under each verdict, as the mockup words it. */
    fun verdictSubtitle(event: ReviewEvent, header: PullHeader?): String = when (event) {
        ReviewEvent.Comment -> "Submit general feedback"
        ReviewEvent.Approve ->
            if (header?.reviewDecision == ReviewDecision.ReviewRequired) {
                "Approving satisfies the required review"
            } else {
                "Give your approval to merge"
            }
        ReviewEvent.RequestChanges -> "Blocks merging until addressed"
    }

    fun verdictTitle(event: ReviewEvent): String = when (event) {
        ReviewEvent.Comment -> "Comment"
        ReviewEvent.Approve -> "Approve"
        ReviewEvent.RequestChanges -> "Request changes"
    }
}
