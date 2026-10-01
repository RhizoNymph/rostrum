package io.github.rhizonymph.rostrum.ui.pr.files

import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.review.CommentTarget
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class DiffSelectionTest {
    private val pr = PrRef("RhizoNymph/rostrum", 10)
    private val overviewRs = "crates/rostrum-diff/src/overview.rs"
    private val filesRs = "crates/rostrum/src/detail/files.rs"

    private suspend fun rows(path: String): List<DiffRow> {
        val backend = testBackend()
        val index = backend.filesOverview(pr).orFail().files.indexOfFirst { it.path == path }
        return (backend.fileDiff(pr, index).orFail().body as FileDiffBody.Rows).rows
    }

    private fun List<DiffRow>.rowOfNew(line: Int) = indexOfFirst { it is DiffRow.Line && it.line.newLine == line && it.line.kind != LineKind.Removed }
    private fun List<DiffRow>.rowOfOld(line: Int) = indexOfFirst { it is DiffRow.Line && it.line.oldLine == line && it.line.kind == LineKind.Removed }

    @Test
    fun `every line with an anchor is a slot, in its hunk`() = runTest {
        val rows = rows(filesRs)
        val slots = DiffSelection.slots(rows)
        assertEquals(rows.count { it is DiffRow.Line }, slots.size)
        assertEquals(0, slots.getValue(rows.rowOfNew(271)).hunk)
        assertEquals(1, slots.getValue(rows.rowOfNew(417)).hunk)
        assertTrue(rows.indices.filter { rows[it] !is DiffRow.Line }.none { it in slots })
    }

    @Test
    fun `hunk headers, threads and drafts cannot start a selection`() = runTest {
        val rows = rows(overviewRs)
        assertNull(DiffSelection.start(rows, 0))
        val thread = rows.indexOfFirst { it is DiffRow.Thread }
        assertNull(DiffSelection.start(rows, thread))
    }

    @Test
    fun `a single line targets itself with no range`() = runTest {
        val rows = rows(overviewRs)
        val selection = DiffSelection.start(rows, rows.rowOfNew(60))!!
        assertEquals(CommentTarget(CommentAnchor(overviewRs, 60, Side.Right)), DiffSelection.target(selection, rows))
    }

    @Test
    fun `dragging down makes a range from the first to the last line`() = runTest {
        val rows = rows(overviewRs)
        val selection = DiffSelection.extend(DiffSelection.start(rows, rows.rowOfNew(53))!!, rows, rows.rowOfNew(56))
        val target = DiffSelection.target(selection, rows)!!
        assertEquals(56, target.line)
        assertEquals(53, target.startLine)
        assertEquals(4, DiffSelection.selectedRows(selection, rows).size)
    }

    @Test
    fun `dragging up works the same way`() = runTest {
        val rows = rows(overviewRs)
        val selection = DiffSelection.extend(DiffSelection.start(rows, rows.rowOfNew(56))!!, rows, rows.rowOfNew(53))
        assertEquals(CommentTarget(CommentAnchor(overviewRs, 56, Side.Right), CommentAnchor(overviewRs, 53, Side.Right)), DiffSelection.target(selection, rows))
    }

    @Test
    fun `a range stays on the side it started on`() = runTest {
        val rows = rows(filesRs)
        val selection = DiffSelection.extend(DiffSelection.start(rows, rows.rowOfOld(264))!!, rows, rows.rowOfNew(266))
        val target = DiffSelection.target(selection, rows)!!
        assertEquals(Side.Left, target.side)
        assertEquals(266, target.line)
        assertEquals(264, target.startLine)
        assertTrue(DiffSelection.selectedRows(selection, rows).none { (rows[it] as DiffRow.Line).line.kind == LineKind.Added })
    }

    @Test
    fun `a range stops at its hunk's edge`() = runTest {
        val rows = rows(filesRs)
        val start = DiffSelection.start(rows, rows.rowOfNew(274))!!
        val selection = DiffSelection.extend(start, rows, rows.rowOfNew(419))
        assertEquals(0, selection.hunk)
        assertEquals(275, DiffSelection.target(selection, rows)!!.line)
    }

    @Test
    fun `rows past the thread still extend the range`() = runTest {
        val rows = rows(overviewRs)
        val selection = DiffSelection.extend(DiffSelection.start(rows, rows.rowOfNew(59))!!, rows, rows.rowOfNew(62))
        val target = DiffSelection.target(selection, rows)!!
        assertEquals(59, target.startLine)
        assertEquals(62, target.line)
    }
}
