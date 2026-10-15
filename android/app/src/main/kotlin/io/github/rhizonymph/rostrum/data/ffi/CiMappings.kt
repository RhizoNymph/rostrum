package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.CiAnnotation
import io.github.rhizonymph.rostrum.data.model.CiAnnotationLevel
import io.github.rhizonymph.rostrum.data.model.CiCell
import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiCheckOutput
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiGrid
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiLineKind
import io.github.rhizonymph.rostrum.data.model.CiLogGroup
import io.github.rhizonymph.rostrum.data.model.CiLogLine
import io.github.rhizonymph.rostrum.data.model.CiLogStep
import io.github.rhizonymph.rostrum.data.model.CiNotRerunnable
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiRerunOption
import io.github.rhizonymph.rostrum.data.model.CiRollup
import io.github.rhizonymph.rostrum.data.model.CiRollupState
import io.github.rhizonymph.rostrum.data.model.CiRow
import io.github.rhizonymph.rostrum.data.model.CiSection
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.data.model.CiStackPlace
import io.github.rhizonymph.rostrum.data.model.CiStatus
import uniffi.rostrum_ffi.CiAnnotation as FfiCiAnnotation
import uniffi.rostrum_ffi.CiAnnotationLevel as FfiCiAnnotationLevel
import uniffi.rostrum_ffi.CiCell as FfiCiCell
import uniffi.rostrum_ffi.CiCheckKey as FfiCiCheckKey
import uniffi.rostrum_ffi.CiCheckOutput as FfiCiCheckOutput
import uniffi.rostrum_ffi.CiColumn as FfiCiColumn
import uniffi.rostrum_ffi.CiGrid as FfiCiGrid
import uniffi.rostrum_ffi.CiGridFilter as FfiCiGridFilter
import uniffi.rostrum_ffi.CiJobLog as FfiCiJobLog
import uniffi.rostrum_ffi.CiLine as FfiCiLine
import uniffi.rostrum_ffi.CiLineKind as FfiCiLineKind
import uniffi.rostrum_ffi.CiLogGroup as FfiCiLogGroup
import uniffi.rostrum_ffi.CiLogLine as FfiCiLogLine
import uniffi.rostrum_ffi.CiLogStep as FfiCiLogStep
import uniffi.rostrum_ffi.CiNotRerunnable as FfiCiNotRerunnable
import uniffi.rostrum_ffi.CiRerun as FfiCiRerun
import uniffi.rostrum_ffi.CiRerunChoice as FfiCiRerunChoice
import uniffi.rostrum_ffi.CiRerunOption as FfiCiRerunOption
import uniffi.rostrum_ffi.CiRollup as FfiCiRollup
import uniffi.rostrum_ffi.CiRollupState as FfiCiRollupState
import uniffi.rostrum_ffi.CiRow as FfiCiRow
import uniffi.rostrum_ffi.CiSection as FfiCiSection
import uniffi.rostrum_ffi.CiSource as FfiCiSource
import uniffi.rostrum_ffi.CiStackPlace as FfiCiStackPlace
import uniffi.rostrum_ffi.CiStatus as FfiCiStatus

/* The CI grid, logs, check output and re-runs: generated records ↔ model. */

internal fun CiGridFilter.toFfi() = FfiCiGridFilter(needsAttention)

internal fun FfiCiGrid.toModel() = CiGrid(
    sections = sections.map { it.toModel() },
    lines = lines.map { it.toModel() },
    ticks = ticks,
    anyRunning = anyRunning,
)

internal fun FfiCiSection.toModel() = CiSection(
    repo = repo,
    columns = columns.map { it.toModel() },
    rows = rows.map { it.toModel() },
    load = load.toModel(),
    hidden = hidden.toInt(),
)

internal fun FfiCiColumn.toModel() = CiColumn(key.toModel(), label)

internal fun FfiCiCheckKey.toModel() = CiCheckKey(workflow, name)

internal fun CiCheckKey.toFfi() = FfiCiCheckKey(workflow, name)

internal fun FfiCiRow.toModel() = CiRow(
    number = number.toInt(),
    title = title,
    headSha = headSha,
    rollup = rollup.toModel(),
    cells = cells.map { it?.toModel() },
    stack = stack?.toModel(),
    fetched = fetched,
    truncated = truncated,
)

internal fun FfiCiRollup.toModel() = CiRollup(
    failing = failing.toInt(),
    running = running.toInt(),
    passing = passing.toInt(),
    other = other.toInt(),
    state = state.toModel(),
    label = label,
    role = role.toModel(),
)

internal fun FfiCiRollupState.toModel(): CiRollupState = when (this) {
    FfiCiRollupState.FAILING -> CiRollupState.Failing
    FfiCiRollupState.RUNNING -> CiRollupState.Running
    FfiCiRollupState.PASSING -> CiRollupState.Passing
    FfiCiRollupState.SETTLED -> CiRollupState.Settled
    FfiCiRollupState.EMPTY -> CiRollupState.Empty
}

internal fun FfiCiStackPlace.toModel(): CiStackPlace = when (this) {
    FfiCiStackPlace.BOTTOM -> CiStackPlace.Bottom
    FfiCiStackPlace.MIDDLE -> CiStackPlace.Middle
    FfiCiStackPlace.TOP -> CiStackPlace.Top
    FfiCiStackPlace.ONLY -> CiStackPlace.Only
}

internal fun FfiCiCell.toModel() = CiCell(
    status = status.toModel(),
    statusLabel = statusLabel,
    role = role.toModel(),
    timingLabel = timingLabel,
    durationLabel = durationLabel,
    ticks = ticks,
    producer = producer,
    detailsUrl = detailsUrl,
    source = source.toModel(),
)

internal fun FfiCiStatus.toModel(): CiStatus = when (this) {
    FfiCiStatus.QUEUED -> CiStatus.Queued
    FfiCiStatus.IN_PROGRESS -> CiStatus.InProgress
    FfiCiStatus.SUCCESS -> CiStatus.Success
    FfiCiStatus.FAILURE -> CiStatus.Failure
    FfiCiStatus.CANCELLED -> CiStatus.Cancelled
    FfiCiStatus.SKIPPED -> CiStatus.Skipped
    FfiCiStatus.NEUTRAL -> CiStatus.Neutral
    FfiCiStatus.TIMED_OUT -> CiStatus.TimedOut
    FfiCiStatus.ACTION_REQUIRED -> CiStatus.ActionRequired
}

internal fun FfiCiSource.toModel(): CiSource = when (this) {
    is FfiCiSource.Actions -> CiSource.Actions(jobId.toLong(), runId.toLong(), runAttempt.toInt())
    is FfiCiSource.App -> CiSource.App(checkRunId.toLong(), app)
    is FfiCiSource.Status -> CiSource.Status
}

internal fun FfiCiLine.toModel(): CiLine = when (this) {
    is FfiCiLine.Header -> CiLine.Header(section.toInt())
    is FfiCiLine.Stack -> CiLine.Stack(section.toInt(), members.toInt())
    is FfiCiLine.Row -> CiLine.Row(section.toInt(), row.toInt())
    is FfiCiLine.Notice -> CiLine.Notice(section.toInt())
    is FfiCiLine.Spacer -> CiLine.Spacer
}

internal fun FfiCiJobLog.toModel() = CiJobLog(
    lines = lines.map { it.toModel() },
    groups = groups.map { it.toModel() },
    steps = steps.map { it.toModel() },
    firstError = firstError?.toInt(),
    failingStep = failingStep?.toInt(),
    collapsed = collapsed.map { it.toInt() },
    dropped = dropped.toInt(),
    truncated = truncated,
)

internal fun FfiCiLogLine.toModel() = CiLogLine(number.toInt(), text, kind.toModel())

internal fun FfiCiLogGroup.toModel() = CiLogGroup(title, header.toInt(), end.toInt())

internal fun FfiCiLogStep.toModel() = CiLogStep(title, start.toInt(), end.toInt())

internal fun FfiCiLineKind.toModel(): CiLineKind = when (this) {
    FfiCiLineKind.PLAIN -> CiLineKind.Plain
    FfiCiLineKind.GROUP_HEADER -> CiLineKind.GroupHeader
    FfiCiLineKind.ERROR -> CiLineKind.Error
    FfiCiLineKind.WARNING -> CiLineKind.Warning
    FfiCiLineKind.NOTICE -> CiLineKind.Notice
    FfiCiLineKind.DEBUG -> CiLineKind.Debug
    FfiCiLineKind.COMMAND -> CiLineKind.Command
}

internal fun FfiCiCheckOutput.toModel() = CiCheckOutput(
    title = title,
    summary = summary.toModel(),
    text = text.toModel(),
    annotations = annotations.map { it.toModel() },
)

internal fun FfiCiAnnotation.toModel() = CiAnnotation(
    path = path,
    startLine = startLine.toInt(),
    endLine = endLine.toInt(),
    level = level.toModel(),
    title = title,
    message = message,
    location = location,
)

internal fun FfiCiAnnotationLevel.toModel(): CiAnnotationLevel = when (this) {
    FfiCiAnnotationLevel.NOTICE -> CiAnnotationLevel.Notice
    FfiCiAnnotationLevel.WARNING -> CiAnnotationLevel.Warning
    FfiCiAnnotationLevel.FAILURE -> CiAnnotationLevel.Failure
}

internal fun FfiCiRerun.toModel(): CiRerun = when (this) {
    is FfiCiRerun.Job -> CiRerun.Job(jobId.toLong())
    is FfiCiRerun.FailedJobs -> CiRerun.FailedJobs(runId.toLong())
    is FfiCiRerun.AllJobs -> CiRerun.AllJobs(runId.toLong())
    is FfiCiRerun.Suite -> CiRerun.Suite(suiteId.toLong())
}

internal fun CiRerun.toFfi(): FfiCiRerun = when (this) {
    is CiRerun.Job -> FfiCiRerun.Job(jobId.toULong())
    is CiRerun.FailedJobs -> FfiCiRerun.FailedJobs(runId.toULong())
    is CiRerun.AllJobs -> FfiCiRerun.AllJobs(runId.toULong())
    is CiRerun.Suite -> FfiCiRerun.Suite(suiteId.toULong())
}

internal fun FfiCiRerunOption.toModel() = CiRerunOption(rerun.toModel(), label, confirmPrompt)

internal fun FfiCiRerunChoice.toModel(): CiRerunChoice = when (this) {
    is FfiCiRerunChoice.Available -> CiRerunChoice.Available(options.map { it.toModel() })
    is FfiCiRerunChoice.Unavailable -> CiRerunChoice.Unavailable(reason.toModel(), message)
}

internal fun FfiCiNotRerunnable.toModel(): CiNotRerunnable = when (this) {
    FfiCiNotRerunnable.STILL_RUNNING -> CiNotRerunnable.StillRunning
    FfiCiNotRerunnable.LEGACY_STATUS -> CiNotRerunnable.LegacyStatus
    FfiCiNotRerunnable.NO_SUITE -> CiNotRerunnable.NoSuite
}
