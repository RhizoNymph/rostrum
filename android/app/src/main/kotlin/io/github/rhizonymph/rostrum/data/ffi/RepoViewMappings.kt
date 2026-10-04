package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.BranchDrift
import io.github.rhizonymph.rostrum.data.model.BranchNote
import io.github.rhizonymph.rostrum.data.model.BranchRow
import io.github.rhizonymph.rostrum.data.model.BranchTree
import io.github.rhizonymph.rostrum.data.model.RepoOverview
import io.github.rhizonymph.rostrum.data.model.TrunkDrift
import io.github.rhizonymph.rostrum.data.model.TrunkSettings
import uniffi.rostrum_ffi.BranchDrift as FfiBranchDrift
import uniffi.rostrum_ffi.BranchNote as FfiBranchNote
import uniffi.rostrum_ffi.BranchRow as FfiBranchRow
import uniffi.rostrum_ffi.BranchTree as FfiBranchTree
import uniffi.rostrum_ffi.RepoOverview as FfiRepoOverview
import uniffi.rostrum_ffi.TrunkDrift as FfiTrunkDrift
import uniffi.rostrum_ffi.TrunkSettings as FfiTrunkSettings

/* The repository screen: generated records → model. */

internal fun FfiRepoOverview.toModel() = RepoOverview(
    repo = repo,
    url = url,
    stars = stars?.toInt(),
    defaultBranch = defaultBranch,
    pulls = pulls.map { it.toModel() },
    issues = issues.map { it.toModel() },
    pullsLoad = pullsLoad.toModel(),
    issuesLoad = issuesLoad.toModel(),
)

internal fun FfiBranchDrift.toModel() = BranchDrift(ahead.toInt(), behind.toInt())

internal fun FfiTrunkDrift.toModel(): TrunkDrift = when (this) {
    is FfiTrunkDrift.Default -> TrunkDrift.Default
    is FfiTrunkDrift.Missing -> TrunkDrift.Missing
    is FfiTrunkDrift.Unknown -> TrunkDrift.Unknown
    is FfiTrunkDrift.Known -> TrunkDrift.Known(drift.toModel())
}

internal fun FfiBranchNote.toModel(): BranchNote = when (this) {
    FfiBranchNote.BREAKS_CYCLE -> BranchNote.BreaksCycle
    FfiBranchNote.AMBIGUOUS_BASE -> BranchNote.AmbiguousBase
}

internal fun FfiBranchRow.toModel(): BranchRow = when (this) {
    is FfiBranchRow.Trunk -> BranchRow.Trunk(name, drift.toModel(), pulls.toInt())
    is FfiBranchRow.OtherBases -> BranchRow.OtherBases
    is FfiBranchRow.Base -> BranchRow.Base(name, pulls.toInt())
    is FfiBranchRow.Pull -> BranchRow.Pull(
        depth = depth.toInt(),
        number = number.toInt(),
        head = head,
        base = base,
        drift = drift?.toModel(),
        note = note?.toModel(),
        stackLabel = stackLabel,
        pull = pull?.toModel(),
    )
}

internal fun FfiTrunkSettings.toModel() = TrunkSettings(detected, configured, existing)

internal fun FfiBranchTree.toModel() = BranchTree(
    repo = repo,
    url = url,
    stars = stars.toInt(),
    defaultBranch = defaultBranch,
    trunks = trunks.toModel(),
    rows = rows.map { it.toModel() },
)
