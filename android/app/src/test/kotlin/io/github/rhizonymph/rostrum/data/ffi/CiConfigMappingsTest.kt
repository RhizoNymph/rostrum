package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiColumnGroup
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiLine
import io.github.rhizonymph.rostrum.data.model.CiLineKind
import io.github.rhizonymph.rostrum.data.model.CiNotRerunnable
import io.github.rhizonymph.rostrum.data.model.CiRepo
import io.github.rhizonymph.rostrum.data.model.CiRerun
import io.github.rhizonymph.rostrum.data.model.CiRerunChoice
import io.github.rhizonymph.rostrum.data.model.CiRollupState
import io.github.rhizonymph.rostrum.data.model.CiSource
import io.github.rhizonymph.rostrum.data.model.CiStackPlace
import io.github.rhizonymph.rostrum.data.model.CiStatus
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.ConfigField
import io.github.rhizonymph.rostrum.data.model.ConfigPushResult
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test
import uniffi.rostrum_ffi.CiCell as FCell
import uniffi.rostrum_ffi.CiCheckKey as FKey
import uniffi.rostrum_ffi.CiColumn as FColumn
import uniffi.rostrum_ffi.CiColumnGroup as FColumnGroup
import uniffi.rostrum_ffi.CiGrid as FGrid
import uniffi.rostrum_ffi.CiJobLog as FJobLog
import uniffi.rostrum_ffi.CiLine as FLine
import uniffi.rostrum_ffi.CiLineKind as FLineKind
import uniffi.rostrum_ffi.CiLogGroup as FGroup
import uniffi.rostrum_ffi.CiLogLine as FLogLine
import uniffi.rostrum_ffi.CiLogStep as FStep
import uniffi.rostrum_ffi.CiNotRerunnable as FNotRerunnable
import uniffi.rostrum_ffi.CiRepo as FRepo
import uniffi.rostrum_ffi.CiRerun as FRerun
import uniffi.rostrum_ffi.CiRerunChoice as FChoice
import uniffi.rostrum_ffi.CiRerunOption as FOption
import uniffi.rostrum_ffi.CiRollup as FRollup
import uniffi.rostrum_ffi.CiRollupState as FRollupState
import uniffi.rostrum_ffi.CiRow as FRow
import uniffi.rostrum_ffi.CiSource as FSource
import uniffi.rostrum_ffi.CiStackPlace as FStackPlace
import uniffi.rostrum_ffi.CiStatus as FStatus
import uniffi.rostrum_ffi.ColorRole as FColorRole
import uniffi.rostrum_ffi.ConfigChange as FChange
import uniffi.rostrum_ffi.ConfigField as FField
import uniffi.rostrum_ffi.ConfigPushResult as FPushResult
import uniffi.rostrum_ffi.DesktopConfigPreview as FPreview
import uniffi.rostrum_ffi.RemoteErrorCode as FRemoteErrorCode
import uniffi.rostrum_ffi.RepoLoad as FRepoLoad
import uniffi.rostrum_ffi.RostrumException
import java.time.Instant

/** The CI grid and config push records, generated → model, and the new errors. */
class CiConfigMappingsTest {
    private val at = Instant.parse("2026-09-28T12:00:00Z")

    private fun cell(status: FStatus, source: FSource) = FCell(
        status, "failure", FColorRole.DANGER, "finished 12m ago", "took 4m 03s", false, "GitHub Actions", "https://ci", source,
    )

    @Test
    fun `a grid maps as one matrix, with groups, repositories, not-run cells and both line kinds`() {
        val grid = FGrid(
            columns = listOf(FColumn(FKey(null, "Coverage"), "Coverage"), FColumn(FKey("CI", "test"), "CI / test")),
            groups = listOf(FColumnGroup(null, 0u, 1u), FColumnGroup("CI", 1u, 2u)),
            rows = listOf(
                FRow(
                    "a/one", 7u, "Fix it", "abc1234",
                    FRollup(1u, 0u, 0u, 0u, FRollupState.FAILING, "1 failing", FColorRole.DANGER),
                    listOf(null, cell(FStatus.FAILURE, FSource.Actions(9_000_000_000uL, 42uL, 2u))),
                    FStackPlace.BOTTOM, true, false,
                ),
            ),
            repos = listOf(FRepo("a/one", FRepoLoad.Loaded(at), 1u, 3u), FRepo("b/two", FRepoLoad.Failed("502", at), 0u, 0u)),
            lines = listOf(FLine.Stack(2u), FLine.Row(0u)),
            hidden = 3u,
            ticks = true,
            anyRunning = false,
        ).toModel()
        assertEquals(CiCheckKey(null, "Coverage"), grid.columns[0].key)
        assertEquals(listOf(CiColumnGroup(null, 0, 1), CiColumnGroup("CI", 1, 2)), grid.groups)
        assertEquals(
            listOf(CiRepo("a/one", RepoLoad.Loaded(at), 1, 3), CiRepo("b/two", RepoLoad.Failed("502", at), 0, 0)),
            grid.repos,
        )
        assertEquals(3, grid.hidden)
        val row = grid.rows.single()
        assertEquals("a/one", row.repo)
        assertEquals(7, row.number)
        assertEquals(CiRollupState.Failing, row.rollup.state)
        assertEquals(CiStackPlace.Bottom, row.stack)
        assertNull(row.cells[0])
        assertEquals(CiStatus.Failure, row.cells[1]?.status)
        assertEquals(ColorRole.Danger, row.cells[1]?.role)
        assertEquals(CiSource.Actions(9_000_000_000L, 42L, 2), row.cells[1]?.source)
        assertEquals(listOf(CiLine.Stack(2), CiLine.Row(0)), grid.lines)
        assertEquals(true, grid.ticks)
    }

    @Test
    fun `sources, statuses and line kinds map one to one`() {
        assertEquals(CiSource.App(5, "Codecov"), FSource.App(5uL, "Codecov").toModel())
        assertEquals(CiSource.Status, FSource.Status.toModel())
        assertEquals(CiStatus.entries.size, FStatus.entries.map { it.toModel() }.toSet().size)
        assertEquals(CiLineKind.entries.size, FLineKind.entries.map { it.toModel() }.toSet().size)
    }

    @Test
    fun `a job log keeps its indices`() {
        val log = FJobLog(
            lines = listOf(FLogLine(101u, "Run tests", FLineKind.GROUP_HEADER), FLogLine(102u, "boom", FLineKind.ERROR)),
            groups = listOf(FGroup("Run tests", 0u, 2u)),
            steps = listOf(FStep("Run tests", 0u, 2u)),
            firstError = 1u,
            failingStep = 0u,
            collapsed = emptyList(),
            dropped = 100u,
            truncated = true,
        ).toModel()
        assertEquals(101, log.lines.first().number)
        assertEquals(CiLineKind.Error, log.lines[1].kind)
        assertEquals(1, log.firstError)
        assertEquals(0, log.failingStep)
        assertEquals(100, log.dropped)
    }

    @Test
    fun `re-runs map both ways and choices keep their reason`() {
        val reruns = listOf(CiRerun.Job(1), CiRerun.FailedJobs(2), CiRerun.AllJobs(3), CiRerun.Suite(4))
        assertEquals(reruns, reruns.map { it.toFfi().toModel() })
        val available = FChoice.Available(listOf(FOption(FRerun.FailedJobs(2uL), "Re-run failed jobs", "Re-run them?"))).toModel()
        assertEquals("Re-run them?", (available as CiRerunChoice.Available).options.single().confirmPrompt)
        val unavailable = FChoice.Unavailable(FNotRerunnable.LEGACY_STATUS, "Only the provider can.").toModel()
        assertEquals(CiNotRerunnable.LegacyStatus, (unavailable as CiRerunChoice.Unavailable).reason)
        assertEquals(true, CiGridFilter(needsAttention = true).toFfi().needsAttention)
    }

    private fun preview(revision: String) = FPreview(
        machine = "framework", repos = listOf("a/one"), added = emptyList(), removed = emptyList(),
        prsPerRepo = 25u, hideDrafts = true, hideEmptyRepos = false, authors = emptyList(), includeInvolved = false,
        autostash = true, changesAnything = true, revision = revision, issuesPerRepo = 40u,
        copyChanges = listOf(FChange(FField.ITEM_SORT, "Item order", "created desc", "updated desc")),
        pushChanges = listOf(FChange(FField.PRS_PER_REPO, "Pull requests per repository", "25", "30")),
    )

    @Test
    fun `the config preview carries its revision and both directions`() {
        val model = preview("r7").toModel()
        assertEquals("r7", model.revision)
        assertEquals(40, model.issuesPerRepo)
        assertEquals(ConfigField.ItemSort, model.copyChanges.single().field)
        assertEquals("30", model.pushChanges.single().after)
        assertEquals(ConfigField.entries.size, FField.entries.map { it.toModel() }.toSet().size)
        assertInstanceOf(ConfigPushResult.Applied::class.java, FPushResult.Applied(preview("r8")).toModel())
        assertEquals("r9", (FPushResult.Changed(preview("r9")).toModel() as ConfigPushResult.Changed).desktop.revision)
    }

    @Test
    fun `the new errors arrive typed`() {
        assertEquals(BackendError.CiNoPermission("scope"), RostrumException.CiNoPermission("scope").toBackendError())
        assertEquals(BackendError.CiNotRerunnable("old"), RostrumException.CiNotRerunnable("old").toBackendError())
        assertEquals(BackendError.CiNotFound, RostrumException.CiNotFound().toBackendError())
        assertEquals(RemoteErrorCode.ConfigChanged, FRemoteErrorCode.CONFIG_CHANGED.toModel())
    }
}
