package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.ConfigChange
import io.github.rhizonymph.rostrum.data.model.ConfigField
import io.github.rhizonymph.rostrum.data.model.ConfigPushResult
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import uniffi.rostrum_ffi.ConfigChange as FfiConfigChange
import uniffi.rostrum_ffi.ConfigField as FfiConfigField
import uniffi.rostrum_ffi.ConfigPushResult as FfiConfigPushResult
import uniffi.rostrum_ffi.DesktopConfigPreview as FfiDesktopConfigPreview

/* Sharing settings with the desktop: the preview, each direction's changes, and a push's result. */

internal fun FfiDesktopConfigPreview.toModel() = DesktopConfigPreview(
    machine = machine,
    repos = repos,
    added = added,
    removed = removed,
    prsPerRepo = prsPerRepo.toInt(),
    hideDrafts = hideDrafts,
    hideEmptyRepos = hideEmptyRepos,
    authors = authors,
    includeInvolved = includeInvolved,
    autostash = autostash,
    changesAnything = changesAnything,
    revision = revision,
    issuesPerRepo = issuesPerRepo?.toInt(),
    copyChanges = copyChanges.map { it.toModel() },
    pushChanges = pushChanges.map { it.toModel() },
)

internal fun FfiConfigChange.toModel() = ConfigChange(field.toModel(), label, before, after)

internal fun FfiConfigField.toModel(): ConfigField = when (this) {
    FfiConfigField.REPOS -> ConfigField.Repos
    FfiConfigField.PRS_PER_REPO -> ConfigField.PrsPerRepo
    FfiConfigField.ISSUES_PER_REPO -> ConfigField.IssuesPerRepo
    FfiConfigField.HIDE_DRAFTS -> ConfigField.HideDrafts
    FfiConfigField.HIDE_EMPTY_REPOS -> ConfigField.HideEmptyRepos
    FfiConfigField.AUTHORS -> ConfigField.Authors
    FfiConfigField.INCLUDE_INVOLVED -> ConfigField.IncludeInvolved
    FfiConfigField.AUTOSTASH -> ConfigField.Autostash
    FfiConfigField.REPO_SORT -> ConfigField.RepoSort
    FfiConfigField.ITEM_SORT -> ConfigField.ItemSort
    FfiConfigField.TRUNKS -> ConfigField.Trunks
}

internal fun FfiConfigPushResult.toModel(): ConfigPushResult = when (this) {
    is FfiConfigPushResult.Applied -> ConfigPushResult.Applied(desktop.toModel())
    is FfiConfigPushResult.Changed -> ConfigPushResult.Changed(desktop.toModel())
}
