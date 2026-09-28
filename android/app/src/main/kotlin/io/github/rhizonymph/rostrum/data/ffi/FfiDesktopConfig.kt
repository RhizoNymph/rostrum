package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import uniffi.rostrum_ffi.DesktopConfigPreview as FfiDesktopConfigPreview

/* "Copy settings from the desktop": the preview record. */

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
)
