package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview

/** One repository of the desktop's list, and whether copying adds it here. */
data class DesktopRepoRow(val repo: String, val added: Boolean)

/** The words of "copy settings from the desktop", shared by pairing and Settings. */
object DesktopConfigText {
    fun offerTitle(machine: String) = "Copy settings from $machine?"

    fun sheetTitle(machine: String) = "Copy settings from $machine"

    fun replaceBody(machine: String) =
        "This replaces this phone's repositories, pull requests per repository, feed filters and stash default with $machine's."

    fun unchanged(machine: String) = "This phone already has $machine's settings."

    fun copiedMessage(machine: String, repositories: Int) = "Copied ${repositories(repositories)} from $machine"

    fun removalWarning(removed: Int): String? =
        if (removed <= 0) null else "${repositories(removed)} will be removed from this phone."

    /** `25 per repository · drafts hidden · empty repositories shown · ada-lin and involved · stash on`. */
    fun preferencesSummary(preview: DesktopConfigPreview): String {
        val authors = when {
            preview.authors.isEmpty() -> "everyone"
            preview.includeInvolved -> preview.authors.joinToString(", ") + " and involved"
            else -> preview.authors.joinToString(", ")
        }
        return listOf(
            "${preview.prsPerRepo} per repository",
            if (preview.hideDrafts) "drafts hidden" else "drafts shown",
            if (preview.hideEmptyRepos) "empty repositories hidden" else "empty repositories shown",
            authors,
            if (preview.autostash) "stash on" else "stash off",
        ).joinToString(" · ")
    }

    /** The desktop's repositories in its order, marking the ones this phone lacks. */
    fun repoRows(preview: DesktopConfigPreview): List<DesktopRepoRow> {
        val added = preview.added.mapTo(mutableSetOf()) { it.lowercase() }
        return preview.repos.map { DesktopRepoRow(it, it.lowercase() in added) }
    }

    private fun repositories(count: Int) = "$count ${if (count == 1) "repository" else "repositories"}"
}
