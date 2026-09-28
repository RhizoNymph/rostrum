package io.github.rhizonymph.rostrum.ui.desktop

import io.github.rhizonymph.rostrum.data.model.CloneInfo
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class DesktopMappingTest {
    @Test
    fun `the last run splits into what needs attention and what updated`() = runTest {
        val run = testBackend().syncAllStatus().orFail()!!
        val view = lastRunView(run)
        assertEquals("Rebase all onto base", view.title)
        assertEquals("6 updated, 1 handed off, 1 refused", view.summary)
        assertEquals(listOf(PrRef("RhizoNymph/rostrum", 10), PrRef("RhizoNymph/rostrum", 9)), view.attention.map { it.pr })
        assertEquals("handed off", view.attention[0].chip!!.text)
        assertEquals(ColorRole.Accent, view.attention[0].chip!!.role)
        assertEquals("worktree has uncommitted changes", view.attention[1].detail)
        assertEquals(6, view.settled.size)
        assertEquals(6, view.updatedCount)
    }

    @Test
    fun `op titles and hints`() {
        assertEquals("Pull all", SyncAllOp.Pull.title)
        assertEquals("pull --rebase", SyncAllOp.Pull.hint)
        assertEquals("Merge base into all", SyncAllOp.MergeBase.title)
        assertEquals("merge origin/<base>", SyncAllOp.MergeBase.hint)
        assertEquals("Rebase all onto base", SyncAllOp.RebaseBase.title)
        assertEquals("rebase origin/<base>", SyncAllOp.RebaseBase.hint)
        assertEquals("Pull all", runTitle(LocalOp.PullRebase))
        assertEquals("Merge remote into all", runTitle(LocalOp.MergeRemote))
    }

    @Test
    fun `machine summary`() {
        val machine = MachineInfo(
            "nymph-desk", "0.1.0", 1,
            listOf(CloneInfo("a/b", "~/a"), CloneInfo("c/d", "~/c")),
            handlerConfigured = true, autostash = false, worktrees = 9,
        )
        assertEquals("Connected · 2 clones · 9 worktrees", machineSummary(machine))
        assertEquals("Connected · 1 clone · 1 worktree", machineSummary(machine.copy(clones = machine.clones.take(1), worktrees = 1)))
    }

    @Test
    fun `handoff meta line`() {
        val session = HandoffSession(
            session = "s", repo = "RhizoNymph/rostrum", number = 10, headRef = "feat/diff-overview",
            worktree = "~/w", startedAt = TEST_NOW.minusSeconds(180), attachCommand = "tmux attach -t =s",
            description = "d", abortLabel = "rebase",
        )
        assertEquals("#10 feat/diff-overview · started 3m ago", handoffMeta(session, TEST_NOW))
        val bare = session.copy(repo = null, number = null, headRef = null, startedAt = null)
        assertEquals("", handoffMeta(bare, TEST_NOW))
        assertTrue(handoffMeta(session.copy(headRef = null), TEST_NOW).startsWith("#10 · started"))
    }

    @Test
    fun `waiting count`() {
        assertEquals("1 waiting", waitingLabel(1))
        assertEquals("3 waiting", waitingLabel(3))
    }
}
