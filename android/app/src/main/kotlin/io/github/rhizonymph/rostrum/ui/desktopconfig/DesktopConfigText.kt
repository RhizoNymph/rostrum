package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.model.ConfigField
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.Settings

/** One repository of the desktop's list, and whether copying adds it here. */
data class DesktopRepoRow(val repo: String, val added: Boolean)

/** The words of "copy settings from the desktop", shared by pairing and Settings. */
object DesktopConfigText {
    fun offerTitle(machine: String) = "Copy settings from $machine?"

    fun sheetTitle(machine: String) = "Copy settings from $machine"

    fun replaceBody(machine: String) =
        "This replaces this profile's repositories, pull requests and issues per repository, feed filters, sorts, trunks and stash default with $machine's."

    fun unchanged(machine: String) = "This profile already has $machine's settings."

    fun copiedMessage(machine: String, repositories: Int) = "Copied ${repositories(repositories)} from $machine"

    fun removalWarning(removed: Int): String? =
        if (removed <= 0) null else "${repositories(removed)} will be removed from this profile."

    /** `25 per repository · drafts hidden · empty repositories shown · ada-lin and involved · stash on`. */
    fun preferencesSummary(preview: DesktopConfigPreview): String {
        val authors = when {
            preview.authors.isEmpty() -> "everyone"
            preview.includeInvolved -> preview.authors.joinToString(", ") + " and involved"
            else -> preview.authors.joinToString(", ")
        }
        val perRepository = preview.issuesPerRepo
            ?.let { "${preview.prsPerRepo} pull requests and $it issues per repository" }
            ?: "${preview.prsPerRepo} per repository"
        return listOf(
            perRepository,
            if (preview.hideDrafts) "drafts hidden" else "drafts shown",
            if (preview.hideEmptyRepos) "empty repositories hidden" else "empty repositories shown",
            authors,
            if (preview.autostash) "stash on" else "stash off",
        ).joinToString(" · ")
    }

    /**
     * What copying would change here, one line each: repositories added and
     * removed, a reorder (which reorders the feed, so the core counts it), and
     * the preferences that differ. Without [phone]'s settings, a change the
     * preview cannot explain is still admitted.
     */
    fun changeLines(preview: DesktopConfigPreview, phone: Settings?): List<String> = buildList {
        if (preview.added.isNotEmpty()) add("Adds ${repositories(preview.added.size)}")
        if (preview.removed.isNotEmpty()) add("Removes ${repositories(preview.removed.size)}")
        val sameSet = preview.added.isEmpty() && preview.removed.isEmpty()
        if (phone == null) {
            if (preview.changesAnything && sameSet) {
                add("Reorders your repositories or changes feed settings to match ${preview.machine}")
            }
            return@buildList
        }
        if (sameSet && phone.repos.map { it.lowercase() } != preview.repos.map { it.lowercase() }) {
            add("Reorders your repositories to match ${preview.machine}")
        }
        val feed = phone.feed
        val feedDiffers = feed.hideDrafts != preview.hideDrafts ||
            feed.hideEmptyRepos != preview.hideEmptyRepos ||
            feed.includeInvolved != preview.includeInvolved ||
            feed.authors.map { it.lowercase() }.toSet() != preview.authors.map { it.lowercase() }.toSet()
        val parts = buildList {
            if (phone.prsPerRepo != preview.prsPerRepo) {
                add("pull requests per repository (${phone.prsPerRepo} → ${preview.prsPerRepo})")
            }
            val issues = preview.issuesPerRepo
            if (issues != null && phone.issuesPerRepo != issues) {
                add("issues per repository (${phone.issuesPerRepo} → $issues)")
            }
            if (feedDiffers) add("feed filters")
            val copied = preview.copyChanges.mapTo(mutableSetOf()) { it.field }
            if (ConfigField.RepoSort in copied || ConfigField.ItemSort in copied) add("sorts")
            if (ConfigField.Trunks in copied) add("trunks")
            if (phone.autostash != preview.autostash) add("the stash default")
        }
        if (parts.isNotEmpty()) add("Changes " + naturalJoin(parts))
    }

    private fun naturalJoin(parts: List<String>): String =
        if (parts.size == 1) parts.single() else parts.dropLast(1).joinToString(", ") + " and " + parts.last()

    /** The desktop's repositories in its order, marking the ones this profile lacks. */
    fun repoRows(preview: DesktopConfigPreview): List<DesktopRepoRow> {
        val added = preview.added.mapTo(mutableSetOf()) { it.lowercase() }
        return preview.repos.map { DesktopRepoRow(it, it.lowercase() in added) }
    }

    private fun repositories(count: Int) = "$count ${if (count == 1) "repository" else "repositories"}"
}
