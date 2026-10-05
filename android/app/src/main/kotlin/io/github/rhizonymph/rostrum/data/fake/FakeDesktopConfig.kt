package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.DesktopConfigApi
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.ConfigChange
import io.github.rhizonymph.rostrum.data.model.ConfigField
import io.github.rhizonymph.rostrum.data.model.ConfigPushResult
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.Settings

/**
 * nymph-desk's shareable settings and their revision, copied to the phone or
 * replaced by the phone's. A push names the revision it previewed; when the
 * desktop changed since ([changeElsewhere]), nothing is written and the
 * answer is [ConfigPushResult.Changed] with the new difference, as the core
 * does.
 */
internal class FakeDesktopConfig(
    private val host: FakeHost,
    private val phone: FakePhoneSettings,
) : DesktopConfigApi {
    /** The shareable part of a config, as both sides hold it. */
    data class Shared(
        val repos: List<String>,
        val prsPerRepo: Int,
        val issuesPerRepo: Int,
        val preferences: FeedPreferences,
        val autostash: Boolean,
    )

    private var desk = Shared(
        repos = SampleDesktop.configRepos,
        prsPerRepo = SampleDesktop.CONFIG_PRS_PER_REPO,
        issuesPerRepo = SampleDesktop.CONFIG_ISSUES_PER_REPO,
        preferences = SampleDesktop.configPreferences,
        autostash = SampleDesktop.CONFIG_AUTOSTASH,
    )
    private var revision = 1

    /** The desktop's settings as the phone would push them now. */
    private fun phoneShared() = Shared(
        repos = phone.repos.toList(),
        prsPerRepo = phone.prsPerRepo,
        issuesPerRepo = phone.issuesPerRepo,
        preferences = phone.preferences,
        autostash = phone.autostash,
    )

    /** For tests and previews: someone changed the desktop's settings (a new revision). */
    fun changeElsewhere(prsPerRepo: Int) {
        desk = desk.copy(prsPerRepo = prsPerRepo)
        revision++
    }

    /** What the desktop holds now, for tests. */
    val desktopSettings: Shared get() = desk

    fun preview(): DesktopConfigPreview {
        val mine = phoneShared()
        val phoneRepos = mine.repos.map { it.lowercase() }.toSet()
        val deskRepos = desk.repos.map { it.lowercase() }.toSet()
        val copyChanges = diff(current = mine, proposed = desk)
        return DesktopConfigPreview(
            machine = SampleDesktop.MACHINE,
            repos = desk.repos,
            added = desk.repos.filter { it.lowercase() !in phoneRepos },
            removed = mine.repos.filter { it.lowercase() !in deskRepos },
            prsPerRepo = desk.prsPerRepo,
            hideDrafts = desk.preferences.hideDrafts,
            hideEmptyRepos = desk.preferences.hideEmptyRepos,
            authors = desk.preferences.authors,
            includeInvolved = desk.preferences.includeInvolved,
            autostash = desk.autostash,
            changesAnything = copyChanges.isNotEmpty(),
            revision = "r$revision",
            issuesPerRepo = desk.issuesPerRepo,
            copyChanges = copyChanges,
            pushChanges = diff(current = desk, proposed = mine),
        )
    }

    override suspend fun desktopConfig(): Outcome<DesktopConfigPreview> = host.call(FakeCall.DesktopConfig) {
        paired { Outcome.Ok(preview()) }
    }

    override suspend fun copyDesktopConfig(): Outcome<Settings> = host.call(FakeCall.CopyDesktopConfig) {
        paired {
            phone.repos.clear()
            phone.repos += desk.repos
            host.adoptRepos(desk.repos)
            phone.prsPerRepo = desk.prsPerRepo
            phone.issuesPerRepo = desk.issuesPerRepo
            phone.preferences = desk.preferences
            phone.autostash = desk.autostash
            host.emitFeed()
            Outcome.Ok(phone.toSettings())
        }
    }

    override suspend fun pushConfigToDesktop(base: String): Outcome<ConfigPushResult> =
        host.call(FakeCall.PushConfigToDesktop) {
            when {
                base.isBlank() -> Outcome.Err(BackendError.InvalidInput("a push needs the revision the preview was made against"))
                !host.isPaired -> Outcome.Err(BackendError.NotPaired)
                base.trim() != "r$revision" -> Outcome.Ok(ConfigPushResult.Changed(preview()))
                else -> {
                    val pushed = phoneShared()
                    if (pushed != desk) {
                        desk = pushed
                        revision++
                    }
                    Outcome.Ok(ConfigPushResult.Applied(preview()))
                }
            }
        }

    private fun <T> paired(block: () -> Outcome<T>): Outcome<T> =
        if (host.isPaired) block() else Outcome.Err(BackendError.NotPaired)

    companion object {
        /** `rostrum_remote::diff`: what replacing [current] with [proposed] changes, in the core's words. */
        fun diff(current: Shared, proposed: Shared): List<ConfigChange> = buildList {
            fun check(field: ConfigField, label: String, before: String, after: String) {
                if (before != after) add(ConfigChange(field, label, before, after))
            }
            fun list(items: List<String>) = if (items.isEmpty()) "(none)" else items.joinToString(", ")
            check(ConfigField.Repos, "Repositories", list(current.repos), list(proposed.repos))
            check(ConfigField.PrsPerRepo, "Pull requests per repository", "${current.prsPerRepo}", "${proposed.prsPerRepo}")
            check(ConfigField.IssuesPerRepo, "Issues per repository", "${current.issuesPerRepo}", "${proposed.issuesPerRepo}")
            val (a, b) = current.preferences to proposed.preferences
            check(ConfigField.HideDrafts, "Hide drafts", "${a.hideDrafts}", "${b.hideDrafts}")
            check(ConfigField.HideEmptyRepos, "Hide empty repositories", "${a.hideEmptyRepos}", "${b.hideEmptyRepos}")
            check(ConfigField.Authors, "Authors", list(a.authors), list(b.authors))
            check(ConfigField.IncludeInvolved, "Include involved", "${a.includeInvolved}", "${b.includeInvolved}")
            check(ConfigField.Autostash, "Autostash", "${current.autostash}", "${proposed.autostash}")
        }
    }
}
