package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.Side

/** The pending review drafts the mockups start with, and the head each was drafted against. */
internal object SampleDrafts {
    class Seeded(val drafts: List<ReviewDraft>, val against: String)

    fun seed(draft: (CommentAnchor, String) -> ReviewDraft): Map<PrRef, Seeded> = mapOf(
        PrRef(SamplePulls.ROSTRUM, 10) to Seeded(
            listOf(
                draft(CommentAnchor("crates/rostrum-diff/src/overview.rs", 60, Side.Right),
                    "A test with a rename-only diff would pin the sliver behaviour."),
                draft(CommentAnchor("crates/rostrum/src/detail/files.rs", 271, Side.Right),
                    "Nit: the toggle bar repeats the tab bar's segment styling."),
            ),
            SamplePulls.DIFF_OVERVIEW_SHA,
        ),
        PrRef(SamplePulls.ROSTRUM, 9) to Seeded(
            listOf(draft(CommentAnchor("src/lib.rs", 21, Side.Right), "Should the roster cap be configurable?")),
            "a41c9e05d3b2a1f0e9d8c7b6a5f4e3d2c1b0a9f8",
        ),
    )
}
