package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.AuthorChip
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.data.model.TabCounts
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Instant

/**
 * The feed's filtering and assembly, as the core does it (`flatten_tab`
 * plus the author filter): per tab, in the saved sorts, a stack sorting as
 * one unit. Pure, so it is unit-tested.
 */
internal object FakeFeedAssembler {
    /** One pull request with the logins it involves besides its author. */
    data class Candidate(val summary: PrSummary, val involved: Set<String>)

    /** One issue with the logins it involves besides its author. */
    data class IssueCandidate(val summary: IssueSummary, val involved: Set<String>)

    /** The fields both kinds are filtered on. */
    private data class Facets(
        val number: Int,
        val title: String,
        val author: String?,
        val labels: List<LabelView>,
        val involved: Set<String>,
        val isDraft: Boolean,
    )

    private fun Candidate.facets() =
        Facets(summary.number, summary.title, summary.author?.login, summary.labels, involved, summary.isDraft)

    private fun IssueCandidate.facets() =
        Facets(summary.number, summary.title, summary.author?.login, summary.labels, involved, isDraft = false)

    fun passes(candidate: Candidate, preferences: FeedPreferences, query: String): Boolean =
        passes(candidate.facets(), preferences, query)

    fun passes(candidate: IssueCandidate, preferences: FeedPreferences, query: String): Boolean =
        passes(candidate.facets(), preferences, query)

    private fun passes(item: Facets, preferences: FeedPreferences, query: String): Boolean {
        if (preferences.hideDrafts && item.isDraft) return false
        if (preferences.authors.isNotEmpty()) {
            val author = item.author?.lowercase()
            val byAuthor = author != null && author in preferences.authors
            val byInvolvement = preferences.includeInvolved && item.involved.any { it in preferences.authors }
            if (!byAuthor && !byInvolvement) return false
        }
        val needle = query.trim().lowercase()
        if (needle.isNotEmpty()) {
            val haystack = buildList {
                add(item.title.lowercase())
                add("#${item.number}")
                item.author?.lowercase()?.let(::add)
                item.labels.forEach { add(it.name.lowercase()) }
            }
            if (haystack.none { needle in it }) return false
        }
        return true
    }

    /** Everything one snapshot is built from. */
    data class Inputs(
        val revision: Long,
        val tab: FeedTab,
        val repos: List<String>,
        val repoFacts: List<FakeSort.RepoFacts>,
        val pulls: Map<String, List<Candidate>>,
        val issues: Map<String, List<IssueCandidate>>,
        val loads: Map<String, RepoLoad>,
        val preferences: FeedPreferences,
        val query: String,
        val collapsed: Set<String>,
        val viewer: UserRef?,
        val sort: FakeSort,
        val stacks: List<FakeStackDef>,
    )

    fun facts(pr: PrSummary) = FakeSort.ItemFacts(pr.number, pr.createdAt, pr.updatedAt, pr.author?.login.orEmpty(), pr.title)

    fun facts(issue: IssueSummary) =
        FakeSort.ItemFacts(issue.number, issue.createdAt, issue.updatedAt, issue.author?.login.orEmpty(), issue.title)

    /** One repository's pull requests as items, stacks grouped, in the item sort. */
    fun pullItems(repo: String, visible: List<PrSummary>, open: List<PrSummary>, sort: FakeSort, stacks: List<FakeStackDef>): List<PullItem> {
        val items = FakeStacks.fold(repo, visible, open, stacks)
        return sort.orderItems(items) { item -> item.pulls.map(::facts) }
    }

    fun issueItems(visible: List<IssueSummary>, sort: FakeSort): List<IssueSummary> =
        sort.orderItems(visible) { listOf(facts(it)) }

    fun assemble(inputs: Inputs): FeedSnapshot = with(inputs) {
        val visiblePulls = repos.associateWith { repo -> pulls[repo].orEmpty().filter { passes(it, preferences, query) } }
        val visibleIssues = repos.associateWith { repo -> issues[repo].orEmpty().filter { passes(it, preferences, query) } }
        val counts = TabCounts(visiblePulls.values.sumOf { it.size }, visibleIssues.values.sumOf { it.size })
        var hidden = 0
        val ordered = sort.orderRepos(repoFacts.filter { it.repo in repos })
        val sections = ordered.mapNotNull { repo ->
            val all = when (tab) {
                FeedTab.PullRequests -> pulls[repo].orEmpty().size
                FeedTab.Issues -> issues[repo].orEmpty().size
            }
            val visibleCount = when (tab) {
                FeedTab.PullRequests -> visiblePulls.getValue(repo).size
                FeedTab.Issues -> visibleIssues.getValue(repo).size
            }
            val load = loads[repo] ?: RepoLoad.Idle
            if (preferences.hideEmptyRepos && load is RepoLoad.Loaded && visibleCount == 0) {
                hidden++
                return@mapNotNull null
            }
            val body = when {
                repo in collapsed -> RepoBody.Collapsed
                all == 0 && (load == RepoLoad.Loading || load == RepoLoad.Idle) -> RepoBody.Loading
                all == 0 && load is RepoLoad.Failed -> RepoBody.Failed(load.reason)
                visibleCount == 0 -> RepoBody.Empty
                tab == FeedTab.PullRequests -> RepoBody.Pulls(
                    pullItems(
                        repo,
                        visiblePulls.getValue(repo).map { it.summary },
                        pulls[repo].orEmpty().map { it.summary },
                        sort,
                        stacks,
                    ),
                )
                else -> RepoBody.Issues(issueItems(visibleIssues.getValue(repo).map { it.summary }, sort))
            }
            RepoSection(repo, load, all, visibleCount, repo in collapsed, body)
        }
        val total = when (tab) {
            FeedTab.PullRequests -> pulls.values.sumOf { it.size }
            FeedTab.Issues -> issues.values.sumOf { it.size }
        }
        FeedSnapshot(
            revision = revision,
            tab = tab,
            tabCounts = counts,
            sort = sort.settings(),
            repos = sections,
            hiddenEmptyRepos = hidden,
            totalOpen = total,
            visibleOpen = counts.of(tab),
            query = query,
            preferences = preferences,
            filterActive = query.isNotBlank() || preferences.hideDrafts || preferences.authors.isNotEmpty(),
            mergeStatesSettling = false,
            viewer = viewer,
        )
    }

    /** Someone the roster counts: their login and when their item last moved. */
    data class Authored(val login: String, val updatedAt: Instant)

    /** You first, then everyone by most recent activity; selected authors are never cut. */
    fun roster(
        items: List<Authored>,
        viewer: String?,
        selected: List<String>,
        limit: Int?,
    ): AuthorRoster {
        val byAuthor = items.groupBy { it.login }
        val ordered = byAuthor.entries
            .sortedWith(
                compareByDescending<Map.Entry<String, List<Authored>>> { it.key.equals(viewer, ignoreCase = true) }
                    .thenByDescending { entry -> entry.value.maxOf { it.updatedAt } },
            )
            .map { (login, authored) ->
                AuthorChip(
                    login = login,
                    avatarUrl = null,
                    openItems = authored.size,
                    isViewer = login.equals(viewer, ignoreCase = true),
                    selected = login.lowercase() in selected,
                )
            }
        if (limit == null) return AuthorRoster(ordered, hidden = 0)
        val kept = mutableListOf<AuthorChip>()
        var unselectedKept = 0
        for (chip in ordered) {
            if (chip.selected || chip.isViewer) {
                kept += chip
            } else if (unselectedKept < limit) {
                kept += chip
                unselectedKept++
            }
        }
        return AuthorRoster(kept, hidden = ordered.size - kept.size)
    }
}
