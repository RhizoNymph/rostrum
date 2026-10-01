package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.BranchRow
import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.IssueCloseReason
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.IssueStatus
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullItem
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.StackKind
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.data.model.TrunkDrift
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class FakeIssuesStacksTest {
    private val rostrum = "RhizoNymph/rostrum"
    private val scroll = IssueRef(rostrum, 21)

    private fun <T> Outcome<T>.error(): BackendError = (this as Outcome.Err).error

    @Nested
    inner class Tabs {
        @Test
        fun `the Issues tab lists issues per repository, filtered like pull requests`() = runTest {
            val backend = testBackend()
            val feed = backend.setFeedTab(FeedTab.Issues).orFail()
            assertEquals(FeedTab.Issues, feed.tab)
            val rostrumIssues = (feed.repos.first { it.repo == rostrum }.body as RepoBody.Issues).issues
            // Created, newest first.
            assertEquals(listOf(21, 18), rostrumIssues.map { it.number })
            // ada-lin is assigned to the zed git issue; wren's vim issue involves nobody chosen.
            val zed = (feed.repos.first { it.repo == "zed-industries/zed" }.body as RepoBody.Issues).issues
            assertEquals(listOf(20410), zed.map { it.number })
            assertEquals(3, feed.tabCounts.issues)
            assertEquals(feed.tabCounts.issues, feed.visibleOpen)
        }

        @Test
        fun `counts cover both tabs whichever is shown`() = runTest {
            val feed = testBackend().clearFilter().orFail()
            assertEquals(FeedTab.PullRequests, feed.tab)
            assertEquals(13, feed.tabCounts.pullRequests)
            assertEquals(4, feed.tabCounts.issues)
        }

        @Test
        fun `the roster counts the active tab's items`() = runTest {
            val backend = testBackend()
            backend.setFeedTab(FeedTab.Issues).orFail()
            val roster = backend.authorRoster(null).orFail()
            assertEquals(setOf("ada-lin", "RhizoNymph", "tjvance", "wren"), roster.authors.map { it.login }.toSet())
            assertTrue(roster.authors.all { it.openItems == 1 })
        }

        @Test
        fun `the item sort applies to issues too, and survives clearing the filter`() = runTest {
            val backend = testBackend()
            backend.setFeedTab(FeedTab.Issues).orFail()
            backend.setItemSort(ItemSortKey.Title, null).orFail()
            val feed = backend.clearFilter().orFail()
            val titles = (feed.repos.first { it.repo == rostrum }.body as RepoBody.Issues).issues.map { it.title }
            assertEquals(titles.sortedBy { it.lowercase() }, titles)
            assertEquals(ItemSortKey.Title, feed.sort.itemKey)
        }
    }

    @Nested
    inner class Stacks {
        @Test
        fun `a stack lists its visible members bottom first under one header`() = runTest {
            val feed = testBackend().clearFilter().orFail()
            val items = (feed.repos.first { it.repo == rostrum }.body as RepoBody.Pulls).items
            val stack = items.filterIsInstance<PullItem.Stack>().single()
            assertEquals(StackKind.GitHub(7), stack.stack.kind)
            assertEquals("Stack 7 · 2 PRs", stack.stack.title)
            assertEquals("main", stack.stack.trunk)
            assertEquals(listOf(9, 11), stack.members.map { it.number })
            assertEquals("0/2 ready", stack.stack.rollup!!.label)
        }

        @Test
        fun `a filter narrows the members, never the grouping`() = runTest {
            val backend = testBackend()
            val feed = backend.setFilter(FeedPreferences.Default.copy(hideDrafts = true)).orFail()
            val stack = (feed.repos.first { it.repo == rostrum }.body as RepoBody.Pulls).items.filterIsInstance<PullItem.Stack>().single()
            assertEquals(listOf(9), stack.members.map { it.number })
            assertEquals(2, stack.stack.memberCount)
        }

        @Test
        fun `a closed member counts as not open`() = runTest {
            val backend = testBackend()
            backend.closePullRequest(PrRef(rostrum, 11)).orFail()
            val feed = backend.clearFilter().orFail()
            val stack = (feed.repos.first { it.repo == rostrum }.body as RepoBody.Pulls).items.filterIsInstance<PullItem.Stack>().single()
            assertEquals(1, stack.stack.absent)
            assertEquals("Stack 7 · 1 PR", stack.stack.title)
        }
    }

    @Nested
    inner class Issues {
        @Test
        fun `the issue screen has the body first, then comments`() = runTest {
            val detail = testBackend().issueDetail(scroll).orFail()
            assertEquals("Feed forgets the scroll position after a background refresh", detail.issue.title)
            assertInstanceOf(TimelineKind.Description::class.java, detail.timeline.first().kind)
            assertEquals(2, detail.timeline.count { it.kind is TimelineKind.Comment })
        }

        @Test
        fun `only a fetched issue is cached`() = runTest {
            val backend = testBackend()
            assertNull(backend.cachedIssueDetail(scroll).orFail())
            backend.issueDetail(scroll).orFail()
            assertEquals(21, backend.cachedIssueDetail(scroll).orFail()!!.issue.number)
        }

        @Test
        fun `closing as not planned drops it from the tab and records the reason`() = runTest {
            val backend = testBackend()
            backend.closeIssue(scroll, CloseIssueAs.NotPlanned).orFail()
            val detail = backend.issueDetail(scroll).orFail()
            assertEquals(IssueStatus.Closed(IssueCloseReason.NotPlanned), detail.issue.status)
            assertEquals("not planned", detail.issue.statusChip.text)
            val event = detail.timeline.last().kind as TimelineKind.Event
            assertEquals(TimelineEvent.ClosedAs(IssueCloseReason.NotPlanned), event.event)
            val feed = backend.setFeedTab(FeedTab.Issues).orFail()
            val numbers = (feed.repos.first { it.repo == rostrum }.body as RepoBody.Issues).issues.map { it.number }
            assertFalse(21 in numbers)
            assertInstanceOf(BackendError.GitHubApi::class.java, backend.closeIssue(scroll, CloseIssueAs.Completed).error())
            backend.reopenIssue(scroll).orFail()
            assertEquals(IssueStatus.Open, backend.issueDetail(scroll).orFail().issue.status)
        }

        @Test
        fun `labels, assignees and comments change the issue and its timeline`() = runTest {
            val backend = testBackend()
            backend.addIssueLabel(scroll, "documentation").orFail()
            backend.removeIssueLabel(scroll, "ui").orFail()
            backend.addIssueAssignee(scroll, "mkowal").orFail()
            backend.removeIssueAssignee(scroll, "RhizoNymph").orFail()
            backend.commentOnIssue(scroll, "On it").orFail()
            val detail = backend.issueDetail(scroll).orFail()
            assertEquals(listOf("bug", "documentation"), detail.issue.labels.map { it.name })
            assertEquals(listOf("mkowal"), detail.issue.assignees.map { it.login })
            assertFalse(detail.issue.assignedToYou)
            assertEquals(3, detail.issue.commentCount)
            val events = detail.timeline.mapNotNull { (it.kind as? TimelineKind.Event)?.event }
            assertTrue(TimelineEvent.Unassigned("RhizoNymph") in events)
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.commentOnIssue(scroll, " ").error())
        }

        @Test
        fun `a new issue takes the next number and needs a title`() = runTest {
            val backend = testBackend()
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.createIssue(rostrum, "  ", "", emptyList(), emptyList()).error())
            val number = backend.createIssue(rostrum, " Crash on rotate ", "steps", listOf("bug", "bug"), listOf("ada-lin")).orFail()
            assertEquals(22, number)
            val issue = backend.issueDetail(IssueRef(rostrum, number)).orFail().issue
            assertEquals("Crash on rotate", issue.title)
            assertEquals(listOf("bug"), issue.labels.map { it.name })
            assertTrue(issue.isYours)
        }

        @Test
        fun `an unknown issue is GitHub's 404`() = runTest {
            assertEquals(404, (testBackend().issueDetail(IssueRef(rostrum, 999)).error() as BackendError.GitHubApi).status)
        }

        @Test
        fun `actions need a token`() = runTest {
            val backend = testBackend(signedIn = false)
            assertEquals(BackendError.NotSignedIn, backend.commentOnIssue(scroll, "x").error())
        }
    }

    @Nested
    inner class RepoScreen {
        @Test
        fun `the overview lists everything, unfiltered, stacks grouped`() = runTest {
            val overview = testBackend().repoOverview(rostrum).orFail()
            assertEquals(listOf(9, 11, 10), overview.pulls.flatMap { it.pulls }.map { it.number })
            assertEquals(listOf(21, 18), overview.issues.map { it.number })
            assertEquals(12, overview.stars)
            assertInstanceOf(BackendError.InvalidInput::class.java, testBackend().repoOverview("x/y").error())
        }

        @Test
        fun `the branch tree nests a stack's top under its bottom`() = runTest {
            val tree = testBackend().branchTree(rostrum).orFail()
            val trunk = tree.rows.first() as BranchRow.Trunk
            assertEquals("main", trunk.name)
            assertEquals(TrunkDrift.Default, trunk.drift)
            assertEquals(2, trunk.pulls)
            val pulls = tree.rows.filterIsInstance<BranchRow.Pull>()
            assertEquals(listOf(9 to 0, 11 to 1, 10 to 0), pulls.map { it.number to it.depth })
            assertEquals("stack 7", pulls.first { it.number == 11 }.stackLabel)
            assertNull(pulls.first { it.number == 10 }.stackLabel)
        }

        @Test
        fun `custom trunks are validated, missing ones flagged, and detection restored`() = runTest {
            val backend = testBackend()
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.setTrunks(rostrum, listOf("main", "bad name")).error())
            val custom = backend.setTrunks(rostrum, listOf("main", "develop", "release")).orFail()
            assertFalse(custom.detected)
            assertEquals(listOf("main", "develop"), custom.existing)
            val tree = backend.branchTree(rostrum).orFail()
            val trunks = tree.rows.filterIsInstance<BranchRow.Trunk>()
            assertEquals(listOf("main", "develop", "release"), trunks.map { it.name })
            assertEquals(TrunkDrift.Missing, trunks.last().drift)
            assertTrue(backend.setTrunks(rostrum, null).orFail().detected)
        }
    }
}
