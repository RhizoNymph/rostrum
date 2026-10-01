package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.AuthorChip
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.data.model.UserRef

/**
 * The feed's filtering and assembly, as the core does it (`rostrum-core`'s
 * feed flattening plus the author filter). Pure, so it is unit-tested.
 */
internal object FakeFeedAssembler {
    /** One pull request with the logins it involves besides its author. */
    data class Candidate(val summary: PrSummary, val involved: Set<String>)

    fun passes(candidate: Candidate, preferences: FeedPreferences, query: String): Boolean {
        val pr = candidate.summary
        if (preferences.hideDrafts && pr.isDraft) return false
        if (preferences.authors.isNotEmpty()) {
            val author = pr.author?.login?.lowercase()
            val byAuthor = author != null && author in preferences.authors
            val byInvolvement = preferences.includeInvolved && candidate.involved.any { it in preferences.authors }
            if (!byAuthor && !byInvolvement) return false
        }
        val needle = query.trim().lowercase()
        if (needle.isNotEmpty()) {
            val haystack = buildList {
                add(pr.title.lowercase())
                add("#${pr.number}")
                pr.author?.login?.lowercase()?.let(::add)
                pr.labels.forEach { add(it.name.lowercase()) }
            }
            if (haystack.none { needle in it }) return false
        }
        return true
    }

    fun assemble(
        revision: Long,
        repos: List<String>,
        candidates: Map<String, List<Candidate>>,
        loads: Map<String, RepoLoad>,
        preferences: FeedPreferences,
        query: String,
        collapsed: Set<String>,
        viewer: UserRef?,
    ): FeedSnapshot {
        var hidden = 0
        val sections = repos.mapNotNull { repo ->
            val all = candidates[repo].orEmpty()
            val visible = all.filter { passes(it, preferences, query) }.map { it.summary }
            val load = loads[repo] ?: RepoLoad.Idle
            val empty = load is RepoLoad.Loaded && visible.isEmpty()
            if (preferences.hideEmptyRepos && empty) {
                hidden++
                return@mapNotNull null
            }
            val body = when {
                repo in collapsed -> RepoBody.Collapsed
                all.isEmpty() && (load == RepoLoad.Loading || load == RepoLoad.Idle) -> RepoBody.Loading
                all.isEmpty() && load is RepoLoad.Failed -> RepoBody.Failed(load.reason)
                visible.isEmpty() -> RepoBody.Empty
                else -> RepoBody.Pulls(visible)
            }
            RepoSection(repo, load, all.size, visible.size, repo in collapsed, body)
        }
        val totalOpen = candidates.values.sumOf { it.size }
        val visibleOpen = repos.sumOf { repo -> candidates[repo].orEmpty().count { passes(it, preferences, query) } }
        return FeedSnapshot(
            revision = revision,
            repos = sections,
            hiddenEmptyRepos = hidden,
            totalOpen = totalOpen,
            visibleOpen = visibleOpen,
            query = query,
            preferences = preferences,
            filterActive = query.isNotBlank() || preferences.hideDrafts || preferences.authors.isNotEmpty(),
            mergeStatesSettling = false,
            viewer = viewer,
        )
    }

    /** You first, then everyone by most recent activity; selected authors are never cut. */
    fun roster(
        pulls: List<PrSummary>,
        viewer: String?,
        selected: List<String>,
        limit: Int?,
    ): AuthorRoster {
        val byAuthor = pulls.filter { it.author != null }.groupBy { it.author!!.login }
        val ordered = byAuthor.entries
            .sortedWith(
                compareByDescending<Map.Entry<String, List<PrSummary>>> { it.key.equals(viewer, ignoreCase = true) }
                    .thenByDescending { entry -> entry.value.maxOf { it.updatedAt } },
            )
            .map { (login, prs) ->
                AuthorChip(
                    login = login,
                    avatarUrl = null,
                    openPrs = prs.size,
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
