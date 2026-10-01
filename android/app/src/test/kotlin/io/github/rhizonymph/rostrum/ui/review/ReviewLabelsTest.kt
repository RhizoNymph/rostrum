package io.github.rhizonymph.rostrum.ui.review

import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DraftAnchor
import io.github.rhizonymph.rostrum.data.model.Side
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows

class ReviewLabelsTest {
    private val path = "crates/rostrum-diff/src/overview.rs"

    @Test
    fun `anchor chips name file, lines and side`() {
        assertEquals("overview.rs · L53–56 · new side", ReviewLabels.anchorChip(path, 53, 56, Side.Right))
        assertEquals("overview.rs · L60 · new side", ReviewLabels.anchorChip(path, null, 60, Side.Right))
        assertEquals("files.rs · L264 · old side", ReviewLabels.anchorChip("a/files.rs", null, 264, Side.Left))
    }

    @Test
    fun `targets and drafts use the same chip`() {
        val target = CommentTarget(CommentAnchor(path, 56, Side.Right), CommentAnchor(path, 53, Side.Right))
        assertEquals("overview.rs · L53–56 · new side", ReviewLabels.anchorChip(target))
        assertEquals("overview.rs · L53–56 · new side", ReviewLabels.anchorChip(DraftAnchor(path, 56, Side.Right, 53)))
    }

    @Test
    fun `locations are short`() {
        assertEquals("overview.rs:L53–56", ReviewLabels.location(DraftAnchor(path, 56, Side.Right, 53)))
        assertEquals("overview.rs:L60", ReviewLabels.location(DraftAnchor(path, 60, Side.Right, null)))
    }

    @Test
    fun `edit and delete descriptions`() {
        assertEquals("Edit comment on overview.rs lines 53–56", ReviewLabels.editDescription(DraftAnchor(path, 56, Side.Right, 53)))
        assertEquals("Edit comment on overview.rs line 60", ReviewLabels.editDescription(DraftAnchor(path, 60, Side.Right, null)))
        assertEquals("Delete comment on overview.rs line 60", ReviewLabels.deleteDescription(DraftAnchor(path, 60, Side.Right, null)))
    }

    @Test
    fun `the anchor note explains the side`() {
        assertEquals("Anchored to new-file lines 53–56 (RIGHT). Removed lines anchor LEFT.", ReviewLabels.anchorNote(53, 56, Side.Right))
        assertEquals("Anchored to new-file line 60 (RIGHT). Removed lines anchor LEFT.", ReviewLabels.anchorNote(null, 60, Side.Right))
        assertEquals(
            "Anchored to old-file line 264 (LEFT). Added and unchanged lines anchor RIGHT.",
            ReviewLabels.anchorNote(null, 264, Side.Left),
        )
    }

    @Test
    fun `pending counts`() {
        assertEquals("1 pending comment", ReviewLabels.pendingCount(1))
        assertEquals("2 pending comments", ReviewLabels.pendingCount(2))
    }

    @Test
    fun `targets refuse ranges across sides, files or upwards`() {
        assertThrows<IllegalArgumentException> { CommentTarget(CommentAnchor(path, 5, Side.Right), CommentAnchor(path, 3, Side.Left)) }
        assertThrows<IllegalArgumentException> { CommentTarget(CommentAnchor(path, 5, Side.Right), CommentAnchor("b", 3, Side.Right)) }
        assertThrows<IllegalArgumentException> { CommentTarget(CommentAnchor(path, 5, Side.Right), CommentAnchor(path, 5, Side.Right)) }
    }
}
