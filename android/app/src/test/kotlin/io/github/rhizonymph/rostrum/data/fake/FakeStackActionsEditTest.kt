package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.StackEligibility
import io.github.rhizonymph.rostrum.data.model.StackJobKind
import io.github.rhizonymph.rostrum.data.model.StackJobResult
import io.github.rhizonymph.rostrum.data.model.StackJobState
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanCheck
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.data.model.StackRewrite
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class FakeStackActionsEditTest {
    private val rostrum = "RhizoNymph/rostrum"

    private fun <T> Outcome<T>.error(): BackendError = (this as Outcome.Err).error

    @Nested
    inner class Stacks {
        @Test
        fun `every action needs a paired desktop`() = runTest {
            val backend = testBackend(paired = false)
            assertEquals(BackendError.NotPaired, backend.mergeStack(rostrum, 7, StackMergeMethod.Merge).error())
            assertEquals(BackendError.NotPaired, backend.planStackRewrite(StackPlanRequest.Extend(rostrum, 7, listOf(10))).error())
            // The local checks need no desktop.
            assertInstanceOf(StackPlanCheck.Valid::class.java, backend.checkStackPlan(StackPlanRequest.Extend(rostrum, 7, listOf(10))).orFail())
        }

        @Test
        fun `candidates for stack 7 are the repository's other pull requests`() = runTest {
            val candidates = testBackend().stackCandidates(rostrum, 7).orFail()
            assertEquals(listOf(10), candidates.map { it.number })
            assertEquals(StackEligibility.Eligible(chained = false), candidates.single().eligibility)
        }

        @Test
        fun `adding #10 rewrites its branch onto the top`() = runTest {
            val plan = testBackend().planStackRewrite(StackPlanRequest.Extend(rostrum, 7, listOf(10))).orFail()
            assertTrue(plan.needsRewrite)
            assertEquals(listOf(StackRewrite(10, "feat/diff-overview")), plan.rewrites)
        }

        @Test
        fun `arranging plans each member onto the one below`() = runTest {
            val backend = testBackend()
            val check = backend.checkStackPlan(StackPlanRequest.Arrange(rostrum, listOf(9, 11), "main")).orFail()
            assertEquals(StackPlanCheck.Valid(emptyList()), check)
            val reordered = backend.checkStackPlan(StackPlanRequest.Arrange(rostrum, listOf(11, 9), "main")).orFail() as StackPlanCheck.Valid
            assertEquals(listOf(11, 9), reordered.rewrites.map { it.number })
            assertInstanceOf(StackPlanCheck.Invalid::class.java, backend.checkStackPlan(StackPlanRequest.Arrange(rostrum, listOf(9), "main")).orFail())
            assertInstanceOf(StackPlanCheck.Invalid::class.java, backend.checkStackPlan(StackPlanRequest.Arrange(rostrum, listOf(9, 99), "main")).orFail())
        }

        @Test
        fun `rewrites run only on exactly the planned branches`() = runTest {
            val backend = testBackend()
            val refused = backend.extendStack(rostrum, 7, listOf(10), emptyList()).error() as BackendError.RewriteNotConfirmed
            assertEquals(listOf(StackRewrite(10, "feat/diff-overview")), refused.branches)
            val job = backend.extendStack(rostrum, 7, listOf(10), listOf("feat/diff-overview")).orFail()
            assertEquals(StackJobKind.Extend, job.kind)
            assertFalse(job.finished)
        }

        @Test
        fun `make stack refuses a set that isn't already a chain`() = runTest {
            val backend = testBackend()
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.makeStack(rostrum, listOf(10, 9), "main").error())
            assertEquals(StackJobKind.Make, backend.makeStack(rostrum, listOf(9, 11), "main").orFail().kind)
        }

        @Test
        fun `a job finishes after polling, with its result or the scripted outcome`() = runTest {
            val backend = testBackend()
            val job = backend.mergeStack(rostrum, 7, StackMergeMethod.Squash).orFail()
            val first = backend.stackJob(job.id).orFail()
            assertInstanceOf(StackJobState.Running::class.java, first.state)
            val done = backend.stackJob(job.id).orFail()
            assertTrue(done.finished)
            assertEquals(StackJobResult.Merged(7), (done.state as StackJobState.Done).result)
            backend.nextStackOutcome = StackJobState.Failed(listOf(9), "push rejected")
            val unstack = backend.unstack(rostrum, 7).orFail()
            backend.stackJob(unstack.id).orFail()
            assertEquals(StackJobState.Failed(listOf(9), "push rejected"), backend.stackJob(unstack.id).orFail().state)
            assertEquals(RemoteErrorCode.NotFound, (backend.stackJob(999).error() as BackendError.RemoteApi).code)
        }
    }

    @Nested
    inner class IssueEditsAndPaging {
        private val scroll = IssueRef(rostrum, 21)

        @Test
        fun `an edit saves the title and description and records the rename`() = runTest {
            val backend = testBackend()
            val opened = backend.issueDetail(scroll).orFail()
            val saved = backend.editIssue(scroll, " Keep the scroll ", "New body", opened.issue.updatedAt, overwrite = false).orFail()
            assertEquals("Keep the scroll", saved.issue.title)
            assertEquals("New body", (saved.timeline.first().kind as TimelineKind.Description).source)
            assertTrue(saved.timeline.any { (it.kind as? TimelineKind.Event)?.event is TimelineEvent.Renamed })
        }

        @Test
        fun `a blank title is refused and an unchanged edit sends nothing`() = runTest {
            val backend = testBackend()
            val opened = backend.issueDetail(scroll).orFail()
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.editIssue(scroll, " ", "x", opened.issue.updatedAt, false).error())
            val body = (opened.timeline.first().kind as TimelineKind.Description).source
            val same = backend.editIssue(scroll, opened.issue.title, body, opened.issue.updatedAt, false).orFail()
            assertEquals(opened.issue.updatedAt, same.issue.updatedAt)
        }

        @Test
        fun `a change on GitHub since opening is a conflict, unless overwriting`() = runTest {
            val backend = testBackend()
            val opened = backend.issueDetail(scroll).orFail()
            backend.editIssueElsewhere(scroll, "Their title", "Their body")
            val conflict = backend.editIssue(scroll, "Mine", "My body", opened.issue.updatedAt, false).error() as BackendError.EditConflict
            assertEquals("Their title", conflict.title)
            assertEquals("Their body", conflict.body)
            val saved = backend.editIssue(scroll, "Mine", "My body", conflict.updatedAt, overwrite = true).orFail()
            assertEquals("Mine", saved.issue.title)
        }

        @Test
        fun `issue #18 has two earlier comments to load`() = runTest {
            val backend = testBackend()
            val ref = IssueRef(rostrum, 18)
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.loadEarlierIssue(ref).error())
            val first = backend.issueDetail(ref).orFail()
            assertTrue(first.hasEarlier)
            assertEquals(2, first.earlierCount)
            val all = backend.loadEarlierIssue(ref).orFail()
            assertFalse(all.hasEarlier)
            assertEquals(first.timeline.size + 2, all.timeline.size)
            assertEquals(listOf("issue-18-old-1", "issue-18-old-2"), all.timeline.drop(1).take(2).map { it.id })
            assertFalse(backend.issueDetail(ref).orFail().hasEarlier)
        }

        @Test
        fun `pull request #9 has two earlier comments to load`() = runTest {
            val backend = testBackend()
            val pr = PrRef(rostrum, 9)
            val first = backend.pullDetail(pr).orFail()
            assertTrue(first.hasEarlier)
            assertEquals(2, first.earlierCount)
            val all = backend.loadEarlierPull(pr).orFail()
            assertFalse(all.hasEarlier)
            assertEquals(first.timeline.size + 2, all.timeline.size)
        }
    }
}
