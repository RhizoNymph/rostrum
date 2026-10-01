package io.github.rhizonymph.rostrum.ui.items

import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.data.model.StackSummary
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import java.time.Instant

/*
 * Pull request, issue and stack rows, shared by the feed and the repository
 * screen: what each row shows, decided here so it is unit-tested.
 */

/** An icon a row chip leads with. */
enum class ChipIcon { Check }

/** One chip under a row's meta line. */
data class RowChip(
    val text: String,
    val role: ColorRole,
    val mono: Boolean = false,
    val icon: ChipIcon? = null,
    val description: String? = null,
)

/**
 * The chips of a pull request row, in the mockup's order: merge trouble,
 * your review, the review verdict, then distance from base.
 * Drafts are marked in the meta line instead.
 */
fun rowChips(pr: PrSummary): List<RowChip> = buildList {
    pr.mergeChip?.let { add(RowChip(it.text, it.role, description = it.tooltip)) }
    if (pr.reviewRequested) add(RowChip("Your review", ColorRole.Accent, description = "Your review is requested"))
    pr.reviewChip?.let {
        val icon = if (pr.reviewDecision == ReviewDecision.Approved) ChipIcon.Check else null
        add(RowChip(it.text, it.role, icon = icon, description = it.tooltip))
    }
    pr.behindChip?.let { add(RowChip(it.text, it.role, mono = true, description = it.tooltip)) }
}

/** `you` for your own pull requests, the author's login otherwise. */
fun authorLabel(pr: PrSummary): String = when {
    pr.isYours -> "you"
    else -> pr.author?.login ?: "ghost"
}

/** How long ago the pull request was opened: `2h`. */
fun ageLabel(pr: PrSummary, now: Instant): String = relativeAge(pr.createdAt, now)

/** How a line count is coloured: zero counts are subdued. */
enum class CountTone { Added, Removed, Zero }

fun additionsTone(count: Int): CountTone = if (count == 0) CountTone.Zero else CountTone.Added

fun deletionsTone(count: Int): CountTone = if (count == 0) CountTone.Zero else CountTone.Removed

/** The letter in a repository's tile: the name's first letter. */
fun repoInitial(repo: String): String =
    repo.substringAfter('/').firstOrNull()?.uppercaseChar()?.toString() ?: "?"

/** `you` for your own issues, the author's login otherwise. */
fun issueAuthorLabel(issue: IssueSummary): String = when {
    issue.isYours -> "you"
    else -> issue.author?.login ?: "ghost"
}

/** Who an issue is assigned to: `ada-lin, mkowal`; empty without assignees. */
fun assigneesText(issue: IssueSummary): String = issue.assignees.joinToString(", ") { it.login }

/** `3 comments`, `1 comment`, or nothing. */
fun commentsText(count: Int): String? = when (count) {
    0 -> null
    1 -> "1 comment"
    else -> "$count comments"
}

/** Where a pull request sits in a stack, for its chain glyph. */
enum class StackPlace {
    /** The only listed member. */
    Only,

    /** The bottom member (listed first). */
    Bottom,
    Middle,

    /** The top member (listed last). */
    Top;

    companion object {
        fun of(index: Int, count: Int): StackPlace = when {
            count <= 1 -> Only
            index == 0 -> Bottom
            index == count - 1 -> Top
            else -> Middle
        }
    }
}

/** One row of a list of pull requests, stacks and issues. */
sealed interface ItemRow {
    val key: String

    data class Pull(val pr: PrSummary, val stack: StackPlace? = null) : ItemRow {
        override val key: String get() = "pr:${pr.repo}#${pr.number}"
    }

    data class StackHeader(val repo: String, val stack: StackSummary, val firstMember: Int?) : ItemRow {
        override val key: String get() = "stack:$repo#${firstMember ?: stack.title}"
    }

    data class Issue(val issue: IssueSummary) : ItemRow {
        override val key: String get() = "issue:${issue.repo}#${issue.number}"
    }
}

/** A stack becomes its header, then its members bottom first; a lone pull request one row. */
fun rowsOf(repo: String, items: List<PullItem>): List<ItemRow> = items.flatMap { item ->
    when (item) {
        is PullItem.Single -> listOf(ItemRow.Pull(item.pull))
        is PullItem.Stack -> buildList<ItemRow> {
            add(ItemRow.StackHeader(repo, item.stack, item.members.firstOrNull()?.number))
            item.members.forEachIndexed { index, pr -> add(ItemRow.Pull(pr, StackPlace.of(index, item.members.size))) }
        }
    }
}

/** `on main · 1 not open`. */
fun stackSubline(stack: StackSummary): String = buildString {
    append("on ${stack.trunk}")
    if (stack.absent > 0) append(" · ${stack.absent} not open")
}
