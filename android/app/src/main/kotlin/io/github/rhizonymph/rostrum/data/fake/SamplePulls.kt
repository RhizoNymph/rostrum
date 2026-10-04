package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import java.time.Duration
import java.time.Instant

/** The people, repositories and pull requests of the mockups. */
internal object SamplePulls {
    const val VIEWER = "RhizoNymph"

    const val ROSTRUM = "RhizoNymph/rostrum"
    const val ZED = "zed-industries/zed"
    const val RUST = "rust-lang/rust"
    const val TOKIO = "tokio-rs/tokio"
    const val BEVY = "bevyengine/bevy"

    val repos = listOf(ROSTRUM, ZED, RUST, TOKIO, BEVY)

    /** Stars per sample repository, for the repository sort. */
    val stars = mapOf(ROSTRUM to 12, ZED to 51_000, RUST to 99_000, TOKIO to 27_000, BEVY to 37_000)

    const val DIFF_OVERVIEW_SHA = "136c158e2b1f4a9d8c7e6f5a4b3c2d1e0f9a8b7c"

    fun labels(repo: String): List<LabelView> = when (repo) {
        ROSTRUM -> listOf(
            LabelView("diff_review", null),
            LabelView("android", 0xFF3DDC84.toInt()),
            LabelView("bug", 0xFFD73A4A.toInt()),
            LabelView("documentation", 0xFF0075CA.toInt()),
            LabelView("enhancement", 0xFFA2EEEF.toInt()),
            LabelView("local_git", null),
            LabelView("ui", 0xFFBFD4F2.toInt()),
        )
        ZED -> listOf(
            LabelView("area:git", 0xFFF9D0C4.toInt()),
            LabelView("area:editor", 0xFFC5DEF5.toInt()),
            LabelView("performance", 0xFFFBCA04.toInt()),
            LabelView("vim", 0xFF5319E7.toInt()),
        )
        RUST -> listOf(
            LabelView("T-compiler", 0xFFBFD4F2.toInt()),
            LabelView("T-rustdoc", 0xFFBFD4F2.toInt()),
            LabelView("S-waiting-on-review", 0xFFD3DDDD.toInt()),
        )
        else -> emptyList()
    }

    fun pulls(now: Instant): List<FakePull> {
        fun ago(minutes: Long): Instant = now.minus(Duration.ofMinutes(minutes))
        val hour = 60L
        val day = 24 * hour
        return listOf(
            FakePull(
                repo = ROSTRUM, number = 10,
                title = "feat: visual diff overview with change map and ranked churn list",
                author = "ada-lin", createdAt = ago(2 * hour), updatedAt = ago(40),
                checks = CheckState.Success, mergeStatus = MergeStatus.Blocked,
                behind = 4, ahead = 3, labels = listOf("diff_review"),
                additions = 900, deletions = 9, changedFiles = 7, comments = 3,
                reviewers = listOf(VIEWER), headRef = "feat/diff-overview", headSha = DIFF_OVERVIEW_SHA,
            ),
            FakePull(
                repo = ROSTRUM, number = 9,
                title = "feat: author filter with involvement, and persisted feed settings",
                author = VIEWER, createdAt = ago(day + 3 * hour), updatedAt = ago(day),
                checks = CheckState.Pending, mergeStatus = MergeStatus.Blocked,
                behind = 3, ahead = 5, labels = listOf("enhancement"),
                additions = 1737, deletions = 97, changedFiles = 14, comments = 1,
                reviewers = listOf("ada-lin"), headRef = "feat/author-filter", headSha = "e7d20b4a1c9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d",
            ),
            FakePull(
                repo = ROSTRUM, number = 11,
                title = "docs: android companion design",
                baseRef = "feat/author-filter",
                author = VIEWER, createdAt = ago(12), updatedAt = ago(12), isDraft = true,
                checks = null, reviewDecision = null, mergeStatus = MergeStatus.Draft,
                behind = 0, ahead = 1, labels = listOf("android", "documentation"),
                additions = 214, deletions = 0, changedFiles = 1,
                headRef = "docs/android-design", headSha = "9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b",
            ),
            FakePull(
                repo = ZED, number = 38112,
                title = "Reduce allocations in the git panel's status refresh",
                author = "tjvance", createdAt = ago(5 * hour), updatedAt = ago(3 * hour),
                checks = CheckState.Failure, mergeStatus = MergeStatus.Conflicts,
                behind = 2, labels = listOf("area:git", "performance"),
                additions = 142, deletions = 97, changedFiles = 6, comments = 4,
                reviewers = listOf(VIEWER, "mkowal"), headRef = "tj/git-status-allocs",
                headSha = "4c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d",
            ),
            FakePull(
                repo = ZED, number = 38090,
                title = "Add a soft-wrap toggle to the project diff",
                author = "mkowal", createdAt = ago(day + 2 * hour), updatedAt = ago(day),
                checks = CheckState.Success, reviewDecision = ReviewDecision.Approved, mergeStatus = MergeStatus.Behind,
                behind = 12, labels = listOf("area:editor"),
                additions = 61, deletions = 8, changedFiles = 3, comments = 2,
                assignees = listOf("ada-lin"), headRef = "mkowal/diff-soft-wrap",
                headSha = "7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f",
            ),
            FakePull(
                repo = ZED, number = 38150,
                title = "terminal: respect OSC 8 hyperlinks",
                author = "sofia-r", createdAt = ago(40), updatedAt = ago(35),
                checks = CheckState.Pending, mergeStatus = MergeStatus.Blocked, behind = 0,
                additions = 88, deletions = 12, changedFiles = 4,
                headRef = "sofia/osc8", headSha = "0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e",
            ),
            FakePull(
                repo = ZED, number = 38120,
                title = "Fix flicker when resizing split panes",
                author = "jmoreau", createdAt = ago(3 * hour), updatedAt = ago(2 * hour),
                checks = CheckState.Success, mergeStatus = MergeStatus.Blocked, behind = 1,
                additions = 23, deletions = 9, changedFiles = 2,
                headRef = "jm/split-flicker", headSha = "5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c",
            ),
            FakePull(
                repo = ZED, number = 38101,
                title = "vim: support gv after visual block",
                author = "wren", createdAt = ago(20 * hour), updatedAt = ago(18 * hour),
                checks = CheckState.Pending, mergeStatus = MergeStatus.Blocked, behind = 5, labels = listOf("vim"),
                additions = 47, deletions = 3, changedFiles = 2,
                headRef = "wren/vim-gv", headSha = "2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b",
            ),
            FakePull(
                repo = ZED, number = 38077,
                title = "Bump tree-sitter-rust to 0.24",
                author = "dmitri-k", createdAt = ago(2 * day), updatedAt = ago(2 * day), isDraft = true,
                checks = CheckState.Success, reviewDecision = null, mergeStatus = MergeStatus.Draft, behind = 9,
                additions = 14, deletions = 14, changedFiles = 3,
                headRef = "dk/ts-rust-024", headSha = "8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a",
            ),
            FakePull(
                repo = RUST, number = 131820,
                title = "Stabilize `precise_capturing_in_traits`",
                author = "kbelov", createdAt = ago(3 * day), updatedAt = ago(6 * hour),
                checks = CheckState.Success, mergeStatus = MergeStatus.Blocked, behind = 40, labels = listOf("T-compiler"),
                additions = 312, deletions = 180, changedFiles = 21,
                headRef = "stabilize-pcit", headSha = "1f2e3d4c5b6a7f8e9d0c1b2a3f4e5d6c7b8a9f0e",
            ),
            FakePull(
                repo = RUST, number = 131799,
                title = "rustdoc: collapse deeply nested impl blocks",
                author = "hanna-s", createdAt = ago(4 * day), updatedAt = ago(day),
                checks = CheckState.Success, mergeStatus = MergeStatus.Blocked, behind = 58, labels = listOf("T-rustdoc"),
                additions = 96, deletions = 31, changedFiles = 5,
                reviewers = listOf("ada-lin"), headRef = "rustdoc-nested-impls",
                headSha = "3e4d5c6b7a8f9e0d1c2b3a4f5e6d7c8b9a0f1e2d",
            ),
            FakePull(
                repo = RUST, number = 131771,
                title = "Improve diagnostics for a missing lifetime in async fn",
                author = "oyelowo", createdAt = ago(5 * day), updatedAt = ago(2 * day),
                checks = CheckState.Failure, mergeStatus = MergeStatus.Unstable, behind = 71, labels = listOf("T-compiler"),
                additions = 58, deletions = 12, changedFiles = 4,
                headRef = "async-lifetime-diag", headSha = "6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b",
            ),
            FakePull(
                repo = RUST, number = 131742,
                title = "Update LLVM to 19.1.3",
                author = "liu-yang", createdAt = ago(6 * day), updatedAt = ago(3 * day),
                checks = CheckState.Pending, mergeStatus = MergeStatus.Blocked, behind = 90,
                additions = 2, deletions = 2, changedFiles = 2,
                headRef = "llvm-19.1.3", headSha = "9d0c1b2a3f4e5d6c7b8a9f0e1d2c3b4a5f6e7d8c",
            ),
        )
    }
}
