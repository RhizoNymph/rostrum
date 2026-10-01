package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.ui.graphics.Color
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.DiffStats
import io.github.rhizonymph.rostrum.data.model.FileStatus
import io.github.rhizonymph.rostrum.data.model.MapTile
import io.github.rhizonymph.rostrum.data.model.RankedFile
import io.github.rhizonymph.rostrum.data.model.TileHeat
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource

class OverviewLayoutTest {
    @Nested
    inner class Distribute {
        @Test
        fun `sizes follow shares and fill the space after gaps`() {
            val sizes = distribute(listOf(0.5f, 0.25f, 0.25f), available = 108f, gap = 4f, min = 8f)
            assertEquals(listOf(50f, 25f, 25f), sizes)
        }

        @Test
        fun `slivers get the minimum and the rest shrink to make room`() {
            val sizes = distribute(listOf(0.447f, 0.41f, 0.096f, 0.04f, 0.007f), available = 346f, gap = 4f, min = 8f)
            assertEquals(330f, sizes.sum(), 0.01f)
            assertEquals(8f, sizes.last(), 0.001f)
            assertTrue(sizes[0] > sizes[1] && sizes[1] > sizes[2] && sizes[2] > sizes[3])
        }

        @Test
        fun `raising one sliver can push another under the minimum`() {
            val sizes = distribute(listOf(0.98f, 0.01f, 0.01f), available = 40f, gap = 0f, min = 10f)
            assertEquals(listOf(20f, 10f, 10f), sizes)
        }

        @Test
        fun `too little room splits evenly`() {
            assertEquals(listOf(5f, 5f), distribute(listOf(0.9f, 0.1f), available = 12f, gap = 2f, min = 8f))
        }

        @Test
        fun `nothing to distribute`() {
            assertEquals(emptyList<Float>(), distribute(emptyList(), 100f, 4f, 8f))
            assertEquals(listOf(50f, 50f), distribute(listOf(0f, 0f), 100f, 0f, 8f))
        }
    }

    @ParameterizedTest
    @CsvSource(
        "150, 160, Stacked", "150, 47, Inline", "138, 18, Inline", "36, 220, Vertical",
        "14, 220, None", "138, 8, None", "40, 40, None",
    )
    fun `tile content follows the room it has`(width: Float, height: Float, expected: TileContent) {
        assertEquals(expected, tileContent(width, height))
    }

    @Test
    fun `narrow columns drop their label`() {
        assertTrue(showsColumnLabel(36f))
        assertFalse(showsColumnLabel(14f))
    }

    @Nested
    inner class TileColour {
        private val added = Color(0xFF3FB950)
        private val removed = Color(0xFFF85149)
        private val neutral = Color(0xFF858DA0)

        /** The blend goes through Oklab, so allow a rounding step per channel. */
        private fun assertNear(expected: Color, actual: Color) {
            assertEquals(expected.red, actual.red, 0.01f)
            assertEquals(expected.green, actual.green, 0.01f)
            assertEquals(expected.blue, actual.blue, 0.01f)
            assertEquals(expected.alpha, actual.alpha, 0.001f)
        }

        @Test
        fun `all additions is the added colour at the heat's alpha`() {
            assertNear(added.copy(alpha = 0.46f), tileColor(TileHeat(0f, 0.46f), added, removed, neutral))
        }

        @Test
        fun `all deletions is the removed colour`() {
            assertNear(removed.copy(alpha = 0.3f), tileColor(TileHeat(1f, 0.3f), added, removed, neutral))
        }

        @Test
        fun `a mix lies between the two`() {
            val mixed = tileColor(TileHeat(0.5f, 1f), added, removed, neutral)
            assertTrue(mixed.red > added.red && mixed.red < removed.red)
            assertTrue(mixed.green < added.green && mixed.green > removed.green)
        }

        @Test
        fun `a file with no line changes is neutral`() {
            assertEquals(neutral.copy(alpha = 0.22f), tileColor(TileHeat(null, 0.22f), added, removed, neutral))
        }
    }

    @Test
    fun `tile descriptions say where and how much`() {
        val tile = MapTile(4, "overview.rs", 320, 0, 0.8f, TileHeat(0f, 0.5f))
        assertEquals("overview.rs in crates/rostrum/src/detail, 320 added", tileDescription(tile, "crates/rostrum/src/detail"))
        val mixed = tile.copy(label = "files.rs", additions = 78, deletions = 8)
        assertEquals("files.rs in crates/x, 78 added, 8 removed", tileDescription(mixed, "crates/x"))
        assertEquals("a.rs, no line changes", tileDescription(tile.copy(label = "a.rs", additions = 0), ""))
    }

    @Nested
    inner class RankedBar {
        private fun file(add: Int, del: Int, addShare: Float, delShare: Float) =
            RankedFile(0, "a", FileStatus.Modified, add, del, addShare, delShare)

        @Test
        fun `parts follow their shares`() {
            assertEquals(BarWidths(210f, 22f), rankedBar(file(78, 8, 0.21f, 0.022f), 1000f).let {
                BarWidths(Math.round(it.additions).toFloat(), Math.round(it.deletions).toFloat())
            })
        }

        @Test
        fun `tiny parts stay visible`() {
            assertEquals(BarWidths(2f, 1f), rankedBar(file(6, 1, 0.001f, 0.0001f), 300f))
        }

        @Test
        fun `absent kinds draw nothing`() {
            assertEquals(BarWidths(300f, 0f), rankedBar(file(371, 0, 1f, 0f), 300f))
        }

        @Test
        fun `never wider than the track`() {
            val bar = rankedBar(file(10, 10, 0.9f, 0.9f), 100f)
            assertEquals(100f, bar.additions + bar.deletions, 0.001f)
        }
    }

    @Test
    fun `status chips per file status`() {
        assertEquals(StatusChipSpec("Added", ColorRole.Success), statusChip(FileStatus.Added))
        assertEquals(StatusChipSpec("Removed", ColorRole.Danger), statusChip(FileStatus.Removed))
        assertEquals(StatusChipSpec("Modified", ColorRole.Accent), statusChip(FileStatus.Modified))
        assertEquals(StatusChipSpec("Renamed", ColorRole.Accent), statusChip(FileStatus.Renamed))
        assertEquals(ColorRole.Neutral, statusChip(FileStatus.Unchanged).role)
    }

    @Test
    fun `the summary lists the kinds present`() {
        assertEquals("3 added · 4 modified", summaryLine(DiffStats(7, 900, 9, 3, 0, 0, 4)))
        assertEquals("1 modified · 2 removed · 1 renamed", summaryLine(DiffStats(4, 1, 1, 0, 2, 1, 1)))
    }
}
