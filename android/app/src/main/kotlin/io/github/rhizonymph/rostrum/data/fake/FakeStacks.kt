package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.StackKind
import io.github.rhizonymph.rostrum.data.model.StackRollup
import io.github.rhizonymph.rostrum.data.model.StackSummary

/** A stack the fake knows: GitHub's (numbered) or a detected chain, members bottom first. */
internal data class FakeStackDef(val repo: String, val kind: StackKind, val trunk: String, val members: List<Int>) {
    /** `stack 7` or `chain`, for the branch tree's chips. */
    val label: String
        get() = when (kind) {
            is StackKind.GitHub -> "stack ${kind.number}"
            StackKind.Chain -> "chain"
        }
}

/** Stacks in the sample data, and folding a repository's visible pull requests into items. */
internal object FakeStacks {
    /** rostrum #9 (feat/author-filter) carries #11 on top of it, as GitHub stack 7. */
    val samples = listOf(
        FakeStackDef(SamplePulls.ROSTRUM, StackKind.GitHub(7), trunk = "main", members = listOf(9, 11)),
    )

    /**
     * Group [visible] (one repository's pull requests in display order) into
     * items: each stack takes the place of its first visible member and
     * lists its visible members bottom first. [open] are every open pull
     * request of the repository, for the member count and the rollup.
     */
    fun fold(repo: String, visible: List<PrSummary>, open: List<PrSummary>, defs: List<FakeStackDef>): List<PullItem> {
        val byNumber = visible.associateBy { it.number }
        val stackOf = mutableMapOf<Int, FakeStackDef>()
        defs.filter { it.repo == repo }.forEach { def -> def.members.forEach { stackOf[it] = def } }
        val done = mutableSetOf<FakeStackDef>()
        return visible.mapNotNull { pr ->
            val def = stackOf[pr.number] ?: return@mapNotNull PullItem.Single(pr)
            if (!done.add(def)) return@mapNotNull null
            val members = def.members.mapNotNull { byNumber[it] }
            PullItem.Stack(summary(def, open.filter { it.number in def.members }), members)
        }
    }

    fun summary(def: FakeStackDef, openMembers: List<PrSummary>): StackSummary {
        val count = openMembers.size
        val title = when (def.kind) {
            is StackKind.GitHub -> "Stack ${def.kind.number} · $count PR${if (count == 1) "" else "s"}"
            StackKind.Chain -> "Stackable chain · $count PRs"
        }
        return StackSummary(
            kind = def.kind,
            title = title,
            trunk = def.trunk,
            memberCount = count,
            absent = def.members.size - count,
            rollup = rollup(openMembers),
        )
    }

    private val ready = setOf(MergeStatus.Ready, MergeStatus.Unstable)

    private fun rollup(members: List<PrSummary>): StackRollup? {
        if (members.isEmpty()) return null
        val mergeable = members.count { it.mergeStatus in ready }
        val worst = members.map { it.mergeStatus }.minBy { severity(it) }
        val label = buildString {
            append("$mergeable/${members.size} ready")
            if (worst == MergeStatus.Conflicts) append(" · conflict")
        }
        val role = when {
            mergeable == members.size -> ColorRole.Success
            worst == MergeStatus.Conflicts -> ColorRole.Danger
            else -> ColorRole.Warning
        }
        return StackRollup(mergeable, members.size, worst, label, role)
    }

    /** Lower is worse. */
    private fun severity(status: MergeStatus): Int = when (status) {
        MergeStatus.Conflicts -> 0
        MergeStatus.Blocked -> 1
        MergeStatus.Behind -> 2
        MergeStatus.Draft -> 3
        MergeStatus.Computing -> 4
        MergeStatus.Unstable -> 5
        MergeStatus.Ready -> 6
    }
}
