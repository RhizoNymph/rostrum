package io.github.rhizonymph.rostrum.ui.pr.common

import io.github.rhizonymph.rostrum.data.model.BaseDivergence
import io.github.rhizonymph.rostrum.data.model.CheckRunView
import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.components.CiShape
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class PrMappingTest {
    private fun run(name: String, state: CheckState?, text: String = "1m") =
        CheckRunView(name, state, ColorRole.Neutral, text, null)

    @Nested
    inner class StateBadges {
        @Test
        fun `open, draft, merged and closed`() {
            assertEquals(StateBadge("Open", ColorRole.Success), stateBadge(PullState.Open, isDraft = false))
            assertEquals(StateBadge("Draft", ColorRole.Draft), stateBadge(PullState.Open, isDraft = true))
            assertEquals(StateBadge("Merged", ColorRole.Accent), stateBadge(PullState.Merged, isDraft = false))
            assertEquals(StateBadge("Closed", ColorRole.Danger), stateBadge(PullState.Closed, isDraft = true))
        }
    }

    @Nested
    inner class Checks {
        @Test
        fun `all passing with one skipped`() {
            val runs = List(6) { run("ci / $it", CheckState.Success) } + run("ci / macos", null, "skipped")
            val summary = checksSummary(runs)
            assertEquals("All checks passed", summary.title)
            assertEquals("6 passed · 1 skipped", summary.subtitle)
            assertEquals(CiShape.Passing, summary.shape)
            assertEquals(ColorRole.Success, summary.role)
        }

        @Test
        fun `failures win over running`() {
            val summary = checksSummary(
                listOf(run("a", CheckState.Failure), run("b", CheckState.Error), run("c", CheckState.Pending), run("d", CheckState.Success)),
            )
            assertEquals("2 failing", summary.title)
            assertEquals("1 passed · 2 failing · 1 running", summary.subtitle)
            assertEquals(CiShape.Failing, summary.shape)
            assertEquals(ColorRole.Danger, summary.role)
        }

        @Test
        fun `running without failures`() {
            val summary = checksSummary(listOf(run("a", CheckState.Pending), run("b", CheckState.Expected)))
            assertEquals("2 running", summary.title)
            assertEquals(CiShape.Running, summary.shape)
            assertEquals(ColorRole.Warning, summary.role)
        }

        @Test
        fun `no checks at all`() {
            val summary = checksSummary(emptyList())
            assertEquals("No checks", summary.title)
            assertEquals("Nothing reported for this commit", summary.subtitle)
            assertEquals(CiShape.None, summary.shape)
        }

        @Test
        fun `a run with no state reads as skipped or no status`() {
            assertEquals(CiShape.Skipped, checkRunShape(run("x", null, "skipped")))
            assertEquals(CiShape.None, checkRunShape(run("x", null, "no status")))
            assertEquals(CiShape.Passing, checkRunShape(run("x", CheckState.Success)))
        }
    }

    @Nested
    inner class Facts {
        @Test
        fun `checks fact counts passing, failing or running`() {
            assertEquals(Fact("Checks", "6 passing", ColorRole.Success, CiShape.Passing), checksFact(List(6) { run("$it", CheckState.Success) }))
            assertEquals("1 failing", checksFact(listOf(run("a", CheckState.Failure))).value)
            assertEquals("1 running", checksFact(listOf(run("a", CheckState.Pending))).value)
            assertEquals(Fact("Checks", "None", ColorRole.Neutral, CiShape.None), checksFact(emptyList()))
        }

        @Test
        fun `reviews fact follows the decision`() {
            assertEquals(Fact("Reviews", "Approved", ColorRole.Success, CiShape.Passing), reviewsFact(ReviewDecision.Approved))
            assertEquals(ColorRole.Danger, reviewsFact(ReviewDecision.ChangesRequested).role)
            assertEquals("Review required", reviewsFact(ReviewDecision.ReviewRequired).value)
            assertEquals("Not required", reviewsFact(null).value)
        }

        @Test
        fun `conflicts fact`() {
            assertEquals(Fact("Conflicts", "None", ColorRole.Success, CiShape.Passing), conflictsFact(MergeStatus.Blocked))
            assertEquals(Fact("Conflicts", "Conflicts", ColorRole.Danger, CiShape.Failing), conflictsFact(MergeStatus.Conflicts))
            assertEquals("Checking…", conflictsFact(MergeStatus.Computing).value)
        }

        @Test
        fun `base fact`() {
            assertEquals(Fact("Base", "Up to date", ColorRole.Success, CiShape.Passing), baseFact(divergence(0)))
            assertEquals(Fact("Base", "↓4 behind main", ColorRole.Warning, CiShape.Running), baseFact(divergence(4)))
            assertEquals("Unknown", baseFact(null).value)
        }

        @Test
        fun `the branch tab shows three facts and the merge sheet four`() = runTest {
            val detail = testBackend().pullDetail(PrRef("RhizoNymph/rostrum", 10)).orFail()
            assertEquals(listOf("Checks", "Reviews", "Conflicts"), branchFacts(detail).map { it.label })
            assertEquals(listOf("Checks", "Reviews", "Conflicts", "Base"), mergeSheetFacts(detail).map { it.label })
            assertEquals("6 passing", branchFacts(detail).first().value)
        }
    }

    @Nested
    inner class Merging {
        @Test
        fun `default commit text per method`() = runTest {
            val header = testBackend().pullHeader(PrRef("RhizoNymph/rostrum", 10)).orFail()
            assertEquals(
                CommitText(
                    "Merge pull request #10 from RhizoNymph/feat/diff-overview",
                    "feat: visual diff overview with change map and ranked churn list",
                ),
                defaultCommit(MergeMethod.Merge, header),
            )
            assertEquals(
                CommitText("feat: visual diff overview with change map and ranked churn list (#10)", ""),
                defaultCommit(MergeMethod.Squash, header),
            )
            assertNull(defaultCommit(MergeMethod.Rebase, header))
        }

        @Test
        fun `method labels`() {
            assertEquals(listOf("Merge commit", "Squash", "Rebase"), MergeMethod.entries.map(::methodLabel))
        }

        @Test
        fun `merge status titles`() = runTest {
            val header = testBackend().pullHeader(PrRef("RhizoNymph/rostrum", 10)).orFail()
            assertEquals("Blocked", mergeStatusTitle(header))
            assertEquals("Merged", mergeStatusTitle(header.copy(state = PullState.Merged)))
            assertEquals("Mergeable", mergeStatusTitle(header.copy(merge = header.merge.copy(status = MergeStatus.Unstable))))
        }

        @Test
        fun `base update explanation`() {
            assertEquals(
                "Update adds main's 4 new commits to feat/diff-overview on GitHub.",
                baseUpdateExplanation(divergence(4), "feat/diff-overview"),
            )
            assertEquals(
                "Update adds main's 1 new commit to x on GitHub.",
                baseUpdateExplanation(divergence(1), "x"),
            )
        }
    }

    private fun divergence(behind: Int) = BaseDivergence(behind, 3, "main", false, "")
}
