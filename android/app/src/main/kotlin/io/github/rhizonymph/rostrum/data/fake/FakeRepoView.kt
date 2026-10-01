package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.BranchDrift
import io.github.rhizonymph.rostrum.data.model.BranchRow
import io.github.rhizonymph.rostrum.data.model.BranchTree
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoOverview
import io.github.rhizonymph.rostrum.data.model.TrunkDrift
import io.github.rhizonymph.rostrum.data.model.TrunkSettings

/**
 * The repository screen's branch tree and trunk setting, as the core builds
 * them: each trunk with its open pull requests beneath, pull requests based
 * on another's head nested under it, other bases after a divider. Pure but
 * for the configured trunks, so it is unit-tested.
 */
internal class FakeRepoView {
    /** Configured trunks per repository; absent means detection. */
    private val configured = mutableMapOf<String, List<String>>()

    /** The branches the fake says exist: `main`, `develop` in rostrum, and every head. */
    private fun branches(repo: String, pulls: List<PrSummary>): Set<String> =
        buildSet {
            add(DEFAULT_BRANCH)
            if (repo == SamplePulls.ROSTRUM) add("develop")
            pulls.forEach { add(it.headRef) }
        }

    /** The repository screen's lists: every open pull request (stacks grouped) and issue, in the item sort. */
    fun overview(repo: String, open: List<PrSummary>, issues: List<IssueSummary>, load: RepoLoad, sort: FakeSort) = RepoOverview(
        repo = repo,
        url = "https://github.com/$repo",
        stars = SamplePulls.stars[repo],
        defaultBranch = DEFAULT_BRANCH,
        pulls = FakeFeedAssembler.pullItems(repo, open, open, sort, FakeStacks.samples),
        issues = FakeFeedAssembler.issueItems(issues, sort),
        pullsLoad = load,
        issuesLoad = load,
    )

    fun trunks(repo: String, pulls: List<PrSummary>): TrunkSettings {
        val names = configured[repo]
        val existing = branches(repo, pulls)
        return TrunkSettings(
            detected = names == null,
            configured = names.orEmpty(),
            existing = (names ?: listOf(DEFAULT_BRANCH)).filter { it in existing },
        )
    }

    fun setTrunks(repo: String, names: List<String>?, pulls: List<PrSummary>): Outcome<TrunkSettings> {
        if (names == null) {
            configured.remove(repo)
            return Outcome.Ok(trunks(repo, pulls))
        }
        val cleaned = names.map { it.trim() }.filter { it.isNotEmpty() }.distinct()
        cleaned.firstOrNull { !validBranchName(it) }?.let {
            return Outcome.Err(BackendError.InvalidInput("$it isn't a branch name"))
        }
        if (cleaned.isEmpty()) return Outcome.Err(BackendError.InvalidInput("Name at least one trunk"))
        configured[repo] = cleaned
        return Outcome.Ok(trunks(repo, pulls))
    }

    fun tree(repo: String, pulls: List<PrSummary>, stacks: List<FakeStackDef>, stars: Int): BranchTree {
        val settings = trunks(repo, pulls)
        val trunkNames = configured[repo] ?: listOf(DEFAULT_BRANCH)
        val existing = branches(repo, pulls)
        val heads = pulls.associateBy { it.headRef }
        val children = pulls.groupBy { it.baseRef }
        val labels = stacks.filter { it.repo == repo }.flatMap { def -> def.members.map { it to def.label } }.toMap()
        val rows = mutableListOf<BranchRow>()
        fun addPulls(base: String, depth: Int) {
            children[base].orEmpty().sortedBy { it.number }.forEach { pr ->
                rows += BranchRow.Pull(
                    depth = depth,
                    number = pr.number,
                    head = pr.headRef,
                    base = pr.baseRef,
                    drift = pr.baseDivergence?.let { BranchDrift(it.ahead, it.behind) },
                    note = null,
                    stackLabel = labels[pr.number],
                    pull = pr,
                )
                addPulls(pr.headRef, depth + 1)
            }
        }
        trunkNames.forEach { name ->
            val drift = when {
                name == DEFAULT_BRANCH -> TrunkDrift.Default
                name !in existing -> TrunkDrift.Missing
                else -> TrunkDrift.Known(BranchDrift(ahead = 2, behind = 5))
            }
            rows += BranchRow.Trunk(name, drift, children[name].orEmpty().size)
            addPulls(name, 0)
        }
        val others = children.keys.filter { it !in trunkNames && it !in heads }.sorted()
        if (others.isNotEmpty()) {
            rows += BranchRow.OtherBases
            others.forEach { base ->
                rows += BranchRow.Base(base, children.getValue(base).size)
                addPulls(base, 0)
            }
        }
        return BranchTree(repo, "https://github.com/$repo", stars, DEFAULT_BRANCH, settings, rows)
    }

    companion object {
        const val DEFAULT_BRANCH = "main"

        fun validBranchName(name: String): Boolean =
            name.isNotBlank() && name.none { it.isWhitespace() || it in "~^:?*[\\" } &&
                ".." !in name && !name.startsWith("/") && !name.endsWith("/") && !name.endsWith(".lock")
    }
}
