package io.github.rhizonymph.rostrum.ui.repo

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.BranchDrift
import io.github.rhizonymph.rostrum.data.model.BranchRow
import io.github.rhizonymph.rostrum.data.model.TrunkDrift
import io.github.rhizonymph.rostrum.data.model.TrunkSettings
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

@OptIn(ExperimentalCoroutinesApi::class)
class RepoViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val backend = testBackend()
    private val rostrum = "RhizoNymph/rostrum"

    private fun TestScope.loaded(): RepoViewModel = RepoViewModel(backend, rostrum, TEST_CLOCK).also { advanceUntilIdle() }

    @Test
    fun `opens on the pull requests, stacks grouped, without asking for branches`() = runTest(main.dispatcher) {
        val vm = loaded()
        assertEquals(RepoTab.Pulls, vm.state.value.tab)
        val overview = vm.state.value.overview.dataOrNull()!!
        assertEquals(listOf(9, 11, 10), overview.pulls.flatMap { it.pulls }.map { it.number })
        assertNull(vm.state.value.branches)
        assertEquals("Pull requests 3", repoTabLabel(RepoTab.Pulls, overview))
        assertEquals("Issues 2", repoTabLabel(RepoTab.Issues, overview))
        assertEquals("Branches", repoTabLabel(RepoTab.Branches, overview))
    }

    @Test
    fun `follows feed changes, such as a new issue`() = runTest(main.dispatcher) {
        val vm = loaded()
        backend.createIssue(rostrum, "New", "", emptyList(), emptyList()).orFail()
        advanceUntilIdle()
        assertEquals(3, vm.state.value.overview.dataOrNull()!!.issues.size)
    }

    @Test
    fun `the branch tree loads when its tab is first shown`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.selectTab(RepoTab.Branches)
        assertEquals(UiState.Loading, vm.state.value.branches)
        advanceUntilIdle()
        val tree = vm.state.value.branches!!.dataOrNull()!!
        assertTrue(tree.rows.first() is BranchRow.Trunk)
    }

    @Test
    fun `a failed branch tree can retry`() = runTest(main.dispatcher) {
        val vm = loaded()
        backend.failNext(FakeCall.BranchTree, BackendError.Network("offline"))
        vm.selectTab(RepoTab.Branches)
        advanceUntilIdle()
        assertInstanceOf(UiState.Error::class.java, vm.state.value.branches)
        vm.retryBranches()
        advanceUntilIdle()
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.branches)
    }

    @Test
    fun `custom trunks are saved and the tree rebuilt, and detection restores`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.selectTab(RepoTab.Branches)
        advanceUntilIdle()
        vm.openTrunkEditor()
        assertEquals(TrunkEditor(detect = true, text = ""), vm.state.value.trunkEditor)
        vm.setTrunkDetect(false)
        vm.onTrunkTextChange("main, develop")
        vm.saveTrunks()
        advanceUntilIdle()
        assertNull(vm.state.value.trunkEditor)
        val trunks = vm.state.value.branches!!.dataOrNull()!!.rows.filterIsInstance<BranchRow.Trunk>()
        assertEquals(listOf("main", "develop"), trunks.map { it.name })
        vm.openTrunkEditor()
        assertEquals(TrunkEditor(detect = false, text = "main, develop"), vm.state.value.trunkEditor)
        vm.setTrunkDetect(true)
        vm.saveTrunks()
        advanceUntilIdle()
        assertTrue(vm.state.value.branches!!.dataOrNull()!!.trunks.detected)
    }

    @Test
    fun `a bad trunk name stays in the editor with the error`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.openTrunkEditor()
        advanceUntilIdle()
        vm.setTrunkDetect(false)
        vm.onTrunkTextChange("main, a..b")
        vm.saveTrunks()
        advanceUntilIdle()
        val editor = vm.state.value.trunkEditor!!
        assertInstanceOf(BackendError.InvalidInput::class.java, (editor.save as ActionState.Failed).error)
        vm.onTrunkTextChange("main")
        assertEquals(ActionState.Idle, vm.state.value.trunkEditor!!.save)
        vm.closeTrunkEditor()
        assertNull(vm.state.value.trunkEditor)
    }

    @Test
    fun `refresh re-fetches the repository`() = runTest(main.dispatcher) {
        val vm = loaded()
        vm.refresh()
        advanceUntilIdle()
        assertFalse(vm.state.value.refreshing)
        assertInstanceOf(UiState.Loaded::class.java, vm.state.value.overview)
    }

    @Test
    fun `trunk names and drift read naturally`() {
        assertEquals(listOf("main", "develop"), parseTrunkNames(" main,develop  main\n"))
        assertEquals("↑2 ↓5", driftText(BranchDrift(2, 5)))
        assertEquals("↓4", driftText(BranchDrift(0, 4)))
        assertEquals("even", driftText(BranchDrift(0, 0)))
        assertEquals("↑2 ↓5 vs main", trunkDriftText(TrunkDrift.Known(BranchDrift(2, 5)), "main"))
        assertEquals("default branch", trunkDriftText(TrunkDrift.Default, "main"))
        assertEquals("Trunks: detected (main)", trunksSummary(TrunkSettings(true, emptyList(), listOf("main"))))
        assertEquals("Trunks: main, develop", trunksSummary(TrunkSettings(false, listOf("main", "develop"), listOf("main"))))
    }
}
