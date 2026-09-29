package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.CloneInfo
import io.github.rhizonymph.rostrum.data.model.DesktopGitHubToken
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.HandoffState
import io.github.rhizonymph.rostrum.data.model.InProgress
import io.github.rhizonymph.rostrum.data.model.InProgressKind
import io.github.rhizonymph.rostrum.data.model.JobOutcome
import io.github.rhizonymph.rostrum.data.model.JobResult
import io.github.rhizonymph.rostrum.data.model.LocalBranch
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.PairingResult
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncEntry
import io.github.rhizonymph.rostrum.data.model.SyncEntryState
import io.github.rhizonymph.rostrum.data.model.SyncRun
import io.github.rhizonymph.rostrum.data.model.SyncSummary
import uniffi.rostrum_ffi.CloneInfo as FfiCloneInfo
import uniffi.rostrum_ffi.DesktopGitHubToken as FfiDesktopGitHubToken
import uniffi.rostrum_ffi.DesktopProbe as FfiDesktopProbe
import uniffi.rostrum_ffi.HandoffSession as FfiHandoffSession
import uniffi.rostrum_ffi.HandoffState as FfiHandoffState
import uniffi.rostrum_ffi.InProgress as FfiInProgress
import uniffi.rostrum_ffi.InProgressKind as FfiInProgressKind
import uniffi.rostrum_ffi.JobOutcome as FfiJobOutcome
import uniffi.rostrum_ffi.JobResult as FfiJobResult
import uniffi.rostrum_ffi.LocalBranch as FfiLocalBranch
import uniffi.rostrum_ffi.LocalOp as FfiLocalOp
import uniffi.rostrum_ffi.LocalStatus as FfiLocalStatus
import uniffi.rostrum_ffi.MachineInfo as FfiMachineInfo
import uniffi.rostrum_ffi.PairingPreview as FfiPairingPreview
import uniffi.rostrum_ffi.PairingResult as FfiPairingResult
import uniffi.rostrum_ffi.RemoteStatus as FfiRemoteStatus
import uniffi.rostrum_ffi.SyncAllOp as FfiSyncAllOp
import uniffi.rostrum_ffi.SyncEntry as FfiSyncEntry
import uniffi.rostrum_ffi.SyncEntryState as FfiSyncEntryState
import uniffi.rostrum_ffi.SyncRun as FfiSyncRun
import uniffi.rostrum_ffi.SyncSummary as FfiSyncSummary

/* The paired desktop: pairing, local status and jobs, sync all, handoffs. */

internal fun FfiPairingPreview.toModel() = PairingPreview(machine, hosts, port.toInt(), fingerprintShort, code)

internal fun FfiDesktopProbe.toModel() = DesktopProbe(
    machine = machine,
    apiVersion = apiVersion.toInt(),
    compatible = compatible,
    host = host,
    port = port.toInt(),
    fingerprint = fingerprint,
    fingerprintShort = fingerprintShort,
)

internal fun FfiCloneInfo.toModel() = CloneInfo(repo, path)

internal fun FfiMachineInfo.toModel() = MachineInfo(
    name = name,
    version = version,
    apiVersion = apiVersion.toInt(),
    clones = clones.map { it.toModel() },
    handlerConfigured = handlerConfigured,
    autostash = autostash,
)

internal fun FfiDesktopGitHubToken.toModel() = DesktopGitHubToken(token, source, host)

internal fun FfiPairingResult.toModel() = PairingResult(
    machine = machine.toModel(),
    endpoint = endpoint,
    deviceId = deviceId,
    deviceToken = deviceToken,
    github = github?.toModel(),
)

internal fun FfiRemoteStatus.toModel(): RemoteStatus = when (this) {
    is FfiRemoteStatus.NotPaired -> RemoteStatus.NotPaired
    is FfiRemoteStatus.Paired -> RemoteStatus.Paired(hosts, port.toInt(), fingerprintShort, currentHost)
}

internal fun FfiInProgressKind.toModel(): InProgressKind = when (this) {
    FfiInProgressKind.REBASE -> InProgressKind.Rebase
    FfiInProgressKind.AM -> InProgressKind.Am
    FfiInProgressKind.MERGE -> InProgressKind.Merge
    FfiInProgressKind.CHERRY_PICK -> InProgressKind.CherryPick
    FfiInProgressKind.REVERT -> InProgressKind.Revert
    FfiInProgressKind.BISECT -> InProgressKind.Bisect
}

internal fun FfiInProgress.toModel() = InProgress(kind.toModel(), description, abortable)

internal fun FfiHandoffState.toModel() = HandoffState(session, running, attachCommand)

internal fun FfiLocalBranch.toModel() = LocalBranch(
    worktree = worktree,
    branch = branch,
    ahead = ahead.toInt(),
    behind = behind.toInt(),
    fetched = fetched,
    blocker = blocker,
    inProgress = inProgress?.toModel(),
    handoff = handoff?.toModel(),
)

internal fun FfiLocalStatus.toModel(): LocalStatus = when (this) {
    is FfiLocalStatus.NotConfigured -> LocalStatus.NotConfigured
    is FfiLocalStatus.NotCheckedOut -> LocalStatus.NotCheckedOut
    is FfiLocalStatus.CheckedOut -> LocalStatus.CheckedOut(branch.toModel())
}

internal fun FfiLocalOp.toModel(): LocalOp = when (this) {
    FfiLocalOp.PULL_REBASE -> LocalOp.PullRebase
    FfiLocalOp.MERGE_REMOTE -> LocalOp.MergeRemote
    FfiLocalOp.MERGE_BASE -> LocalOp.MergeBase
    FfiLocalOp.REBASE_BASE -> LocalOp.RebaseBase
}

internal fun LocalOp.toFfi(): FfiLocalOp = when (this) {
    LocalOp.PullRebase -> FfiLocalOp.PULL_REBASE
    LocalOp.MergeRemote -> FfiLocalOp.MERGE_REMOTE
    LocalOp.MergeBase -> FfiLocalOp.MERGE_BASE
    LocalOp.RebaseBase -> FfiLocalOp.REBASE_BASE
}

internal fun SyncAllOp.toFfi(): FfiSyncAllOp = when (this) {
    SyncAllOp.Pull -> FfiSyncAllOp.PULL
    SyncAllOp.MergeBase -> FfiSyncAllOp.MERGE_BASE
    SyncAllOp.RebaseBase -> FfiSyncAllOp.REBASE_BASE
}

internal fun FfiJobOutcome.toModel(): JobOutcome = when (this) {
    is FfiJobOutcome.NotConfigured -> JobOutcome.NotConfigured
    is FfiJobOutcome.NotCheckedOut -> JobOutcome.NotCheckedOut
    is FfiJobOutcome.UpToDate -> JobOutcome.UpToDate
    is FfiJobOutcome.Completed -> JobOutcome.Completed
    is FfiJobOutcome.Refused -> JobOutcome.Refused(reason)
    is FfiJobOutcome.Conflicted -> JobOutcome.Conflicted(reason)
    is FfiJobOutcome.HandedOff -> JobOutcome.HandedOff(session, attachCommand)
    is FfiJobOutcome.Failed -> JobOutcome.Failed(reason)
}

internal fun FfiJobResult.toModel() = JobResult(outcome.toModel(), detail, chip?.toModel())

internal fun FfiSyncEntryState.toModel(): SyncEntryState = when (this) {
    is FfiSyncEntryState.Pending -> SyncEntryState.Pending
    is FfiSyncEntryState.Running -> SyncEntryState.Running
    is FfiSyncEntryState.Done -> SyncEntryState.Done(result.toModel())
}

internal fun FfiSyncEntry.toModel() = SyncEntry(repo, number.toInt(), headRef, state.toModel())

internal fun FfiSyncSummary.toModel() = SyncSummary(
    total = total.toInt(),
    done = done.toInt(),
    updated = updated.toInt(),
    upToDate = upToDate.toInt(),
    handedOff = handedOff.toInt(),
    conflicts = conflicts.toInt(),
    refused = refused.toInt(),
    failed = failed.toInt(),
    skipped = skipped.toInt(),
)

internal fun FfiSyncRun.toModel() = SyncRun(
    id = id.toLong(),
    op = op.toModel(),
    startedAt = startedAt,
    finishedAt = finishedAt,
    entries = entries.map { it.toModel() },
    summary = summary.toModel(),
    progressText = progressText,
)

internal fun FfiHandoffSession.toModel() = HandoffSession(
    session = session,
    repo = repo,
    number = number?.toInt(),
    headRef = headRef,
    worktree = worktree,
    startedAt = startedAt,
    attachCommand = attachCommand,
)
