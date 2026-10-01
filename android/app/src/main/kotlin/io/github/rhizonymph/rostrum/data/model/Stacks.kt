package io.github.rhizonymph.rostrum.data.model

/** Where a stack comes from. */
sealed interface StackKind {
    /** A stack GitHub knows (`gh stack`), with its number. */
    data class GitHub(val number: Int) : StackKind

    /** A chain of pull requests rostrum detected from their bases. */
    data object Chain : StackKind
}

/** The merge states of a stack's members, rolled up for its header. */
data class StackRollup(
    val mergeable: Int,
    val total: Int,
    val worst: MergeStatus,
    /** `2/3 ready`. */
    val label: String,
    val role: ColorRole,
)

/** A stack's header: `Stack 7 · 3 PRs` on trunk `main`. */
data class StackSummary(
    val kind: StackKind,
    val title: String,
    val trunk: String,
    val memberCount: Int,
    /** Members that are merged or closed, so not listed. */
    val absent: Int,
    val rollup: StackRollup?,
)
