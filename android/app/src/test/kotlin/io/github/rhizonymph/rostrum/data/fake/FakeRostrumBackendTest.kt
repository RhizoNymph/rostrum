package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.JobOutcome
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

class FakeRostrumBackendTest {
    private val diffOverview = PrRef("RhizoNymph/rostrum", 10)
    private val authorFilter = PrRef("RhizoNymph/rostrum", 9)

    private fun <T> Outcome<T>.error(): BackendError = (this as Outcome.Err).error

    @Nested
    inner class Feed {
        @Test
        fun `the cached feed matches the mockups' filter`() = runTest {
            val feed = testBackend().cachedFeed().orFail()
            assertEquals(listOf("rhizonymph", "ada-lin"), feed.preferences.authors)
            assertTrue(feed.filterActive)
            val rostrum = feed.repos.first { it.repo == "RhizoNymph/rostrum" }
            val numbers = (rostrum.body as RepoBody.Pulls).pulls.map { it.number }
            assertEquals(listOf(10, 9, 11), numbers)
            assertEquals(RepoBody.Collapsed, feed.repos.first { it.repo == "rust-lang/rust" }.body)
            assertEquals(2, feed.hiddenEmptyRepos)
        }

        @Test
        fun `involvement lets in pull requests the chosen authors review or are assigned`() = runTest {
            val feed = testBackend().cachedFeed().orFail()
            val zed = feed.repos.first { it.repo == "zed-industries/zed" }
            assertEquals(setOf(38112, 38090), (zed.body as RepoBody.Pulls).pulls.map { it.number }.toSet())
        }

        @Test
        fun `clearing the filter shows everyone`() = runTest {
            val backend = testBackend()
            val feed = backend.clearFilter().orFail()
            assertFalse(feed.filterActive)
            assertEquals(feed.totalOpen, feed.visibleOpen)
        }

        @Test
        fun `the query matches titles, numbers, authors and labels`() = runTest {
            val backend = testBackend()
            backend.clearFilter()
            assertEquals(1, backend.setQuery("soft-wrap").orFail().visibleOpen)
            assertEquals(1, backend.setQuery("#38112").orFail().visibleOpen)
            assertEquals(1, backend.setQuery("TJVANCE").orFail().visibleOpen)
            assertEquals(2, backend.setQuery("t-compiler").orFail().visibleOpen)
        }

        @Test
        fun `hiding drafts removes them`() = runTest {
            val backend = testBackend()
            val feed = backend.setFilter(FeedPreferences.Default.copy(hideDrafts = true)).orFail()
            val all = feed.repos.flatMap { (it.body as? RepoBody.Pulls)?.pulls.orEmpty() }
            assertTrue(all.none { it.isDraft })
        }

        @Test
        fun `toggling an author is case-insensitive`() = runTest {
            val backend = testBackend()
            assertEquals(listOf("rhizonymph"), backend.toggleAuthor("Ada-Lin").orFail().preferences.authors)
            assertEquals(listOf("rhizonymph", "tjvance"), backend.toggleAuthor("TJVANCE").orFail().preferences.authors)
        }

        @Test
        fun `every change is published with a higher revision`() = runTest {
            val backend = testBackend()
            val first = backend.cachedFeed().orFail().revision
            val published = launch { assertTrue(backend.feedUpdates.first().revision > first) }
            testScheduler.runCurrent()
            backend.toggleCollapsed("rust-lang/rust")
            published.join()
        }

        @Test
        fun `refreshing needs a token`() = runTest {
            assertEquals(BackendError.NotSignedIn, testBackend(signedIn = false).refreshFeed().error())
        }

        @Test
        fun `the roster puts you first and keeps selected authors past the cap`() = runTest {
            val roster = testBackend().authorRoster(limit = 2).orFail()
            assertEquals("RhizoNymph", roster.authors.first().login)
            assertTrue(roster.authors.first().isViewer)
            assertTrue(roster.authors.any { it.login == "ada-lin" && it.selected })
            assertTrue(roster.hidden > 0)
            assertEquals(roster.hidden + roster.authors.size, testBackend().authorRoster(null).orFail().authors.size)
        }
    }

    @Nested
    inner class Settings {
        @Test
        fun `adding accepts owner-name and URLs`() = runTest {
            val backend = testBackend()
            assertEquals("serde-rs/serde", backend.addRepo(" https://github.com/serde-rs/serde.git ").orFail())
            assertEquals("hyperium/hyper", backend.addRepo("github.com/hyperium/hyper/pulls").orFail())
            assertTrue("serde-rs/serde" in backend.settings().orFail().repos)
        }

        @Test
        fun `adding rejects malformed input and duplicates`() = runTest {
            val backend = testBackend()
            assertInstanceOf(BackendError.InvalidRepo::class.java, backend.addRepo("not a repo").error())
            assertEquals(BackendError.DuplicateRepo("rust-lang/rust"), backend.addRepo("Rust-Lang/Rust").error())
        }

        @Test
        fun `removing reports whether it was watched`() = runTest {
            val backend = testBackend()
            assertTrue(backend.removeRepo("tokio-rs/tokio").orFail())
            assertFalse(backend.removeRepo("tokio-rs/tokio").orFail())
        }

        @Test
        fun `intervals are clamped`() = runTest {
            assertEquals(10, testBackend().setRefreshInterval(1).orFail().refreshIntervalSecs)
        }
    }

    @Nested
    inner class PullRequests {
        @Test
        fun `the sample conversation has its description, thread and push`() = runTest {
            val detail = testBackend().pullDetail(diffOverview).orFail()
            assertInstanceOf(TimelineKind.Description::class.java, detail.timeline.first().kind)
            assertEquals(1, detail.unresolvedThreads)
            assertEquals(2, detail.threads.single().excerpt.size)
            assertEquals(7, detail.checks.size)
            assertEquals(MergeStatus.Blocked, detail.header.merge.status)
            assertTrue(detail.header.merge.blocksMerge)
        }

        @Test
        fun `merging is refused while blocked, and allowed once approved`() = runTest {
            val backend = testBackend()
            val head = backend.pullHeader(diffOverview).orFail().headSha
            assertInstanceOf(BackendError.MergeBlocked::class.java, backend.merge(diffOverview, MergeMethod.Squash, null, null, head).error())
            backend.submitReview(diffOverview, ReviewEvent.Approve, "", includeDrafts = false).orFail()
            assertEquals(MergeStatus.Ready, backend.pullHeader(diffOverview).orFail().merge.status)
            backend.merge(diffOverview, MergeMethod.Squash, "t", "m", head).orFail()
            assertEquals(PullState.Merged, backend.pullHeader(diffOverview).orFail().state)
        }

        @Test
        fun `merging against a moved head is refused`() = runTest {
            val backend = testBackend()
            backend.submitReview(diffOverview, ReviewEvent.Approve, "", includeDrafts = false)
            assertInstanceOf(BackendError.MergeBlocked::class.java, backend.merge(diffOverview, MergeMethod.Merge, null, null, "0000000").error())
        }

        @Test
        fun `close and reopen round trip`() = runTest {
            val backend = testBackend()
            backend.closePullRequest(diffOverview).orFail()
            assertEquals(PullState.Closed, backend.pullHeader(diffOverview).orFail().state)
            assertInstanceOf(BackendError.GitHubApi::class.java, backend.closePullRequest(diffOverview).error())
            backend.reopenPullRequest(diffOverview).orFail()
            assertEquals(PullState.Open, backend.pullHeader(diffOverview).orFail().state)
        }

        @Test
        fun `the draft action flips`() = runTest {
            val backend = testBackend()
            val draft = PrRef("RhizoNymph/rostrum", 11)
            val action = backend.pullHeader(draft).orFail().draftAction
            assertFalse(action.toDraft)
            backend.setDraft(draft, action.toDraft).orFail()
            assertTrue(backend.pullHeader(draft).orFail().draftAction.toDraft)
        }

        @Test
        fun `labels add and remove`() = runTest {
            val backend = testBackend()
            backend.addLabel(diffOverview, "ui").orFail()
            assertTrue(backend.pullHeader(diffOverview).orFail().labels.any { it.name == "ui" })
            backend.removeLabel(diffOverview, "ui").orFail()
            assertFalse(backend.pullHeader(diffOverview).orFail().labels.any { it.name == "ui" })
        }

        @Test
        fun `comments and replies land in the conversation`() = runTest {
            val backend = testBackend()
            backend.addComment(diffOverview, "Looks **good**").orFail()
            val thread = backend.pullDetail(diffOverview).orFail().threads.single()
            backend.replyToThread(diffOverview, thread.id, "Agreed").orFail()
            val detail = backend.pullDetail(diffOverview).orFail()
            assertInstanceOf(TimelineKind.Comment::class.java, detail.timeline.last().kind)
            assertEquals(3, detail.threads.single().comments.size)
        }

        @Test
        fun `updating the branch moves the head and makes drafts stale`() = runTest {
            val backend = testBackend()
            val head = backend.pullHeader(diffOverview).orFail().headSha
            backend.updateBranch(diffOverview, BranchUpdateMethod.Rebase, head).orFail()
            val header = backend.pullHeader(diffOverview).orFail()
            assertEquals(0, header.divergence!!.behind)
            assertTrue(backend.pendingReview(diffOverview).orFail().stale)
        }
    }

    @Nested
    inner class Review {
        @Test
        fun `the overview ranks files and its shares sum to one`() = runTest {
            val overview = testBackend().filesOverview(diffOverview).orFail()
            assertEquals(7, overview.stats.files)
            assertEquals(900, overview.stats.additions)
            assertEquals("crates/rostrum-diff/src/overview.rs", overview.ranked.first().path)
            assertEquals(1f, overview.changeMap.sumOf { it.share.toDouble() }.toFloat(), 0.001f)
            overview.changeMap.forEach { assertEquals(1f, it.tiles.sumOf { t -> t.share.toDouble() }.toFloat(), 0.001f) }
        }

        @Test
        fun `a file's diff places the thread and draft after line 60`() = runTest {
            val backend = testBackend()
            val index = backend.filesOverview(diffOverview).orFail().files.indexOfFirst { it.path.endsWith("rostrum-diff/src/overview.rs") }
            val rows = (backend.fileDiff(diffOverview, index).orFail().body as FileDiffBody.Rows).rows
            val line60 = rows.indexOfFirst { it is DiffRow.Line && it.line.newLine == 60 }
            assertInstanceOf(DiffRow.Thread::class.java, rows[line60 + 1])
            assertInstanceOf(DiffRow.Draft::class.java, rows[line60 + 2])
        }

        @Test
        fun `removed lines anchor on the left`() = runTest {
            val backend = testBackend()
            val index = backend.filesOverview(diffOverview).orFail().files.indexOfFirst { it.path.endsWith("detail/files.rs") }
            val rows = (backend.fileDiff(diffOverview, index).orFail().body as FileDiffBody.Rows).rows
            val removed = rows.filterIsInstance<DiffRow.Line>().first { it.line.kind == LineKind.Removed }.line
            assertEquals(Side.Left, removed.anchor!!.side)
            assertEquals(removed.oldLine, removed.anchor!!.line)
            assertNull(removed.newLine)
        }

        @Test
        fun `drafts add, edit and remove`() = runTest {
            val backend = testBackend()
            val anchor = CommentAnchor("crates/rostrum-diff/src/overview.rs", 56, Side.Right)
            val start = CommentAnchor("crates/rostrum-diff/src/overview.rs", 53, Side.Right)
            val added = backend.addDraft(diffOverview, anchor, start, "Methods on DiffFile?").orFail()
            val draft = added.drafts.last()
            assertEquals(53, draft.anchor.startLine)
            assertEquals(3, added.drafts.size)
            assertEquals("Edited", backend.editDraft(diffOverview, draft.id, "Edited").orFail().drafts.last().body)
            assertEquals(2, backend.removeDraft(diffOverview, draft.id).orFail().drafts.size)
        }

        @Test
        fun `anchors outside the diff and backwards ranges are refused`() = runTest {
            val backend = testBackend()
            val path = "crates/rostrum-diff/src/overview.rs"
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.addDraft(diffOverview, CommentAnchor(path, 999, Side.Right), null, "x").error())
            assertInstanceOf(
                BackendError.InvalidInput::class.java,
                backend.addDraft(diffOverview, CommentAnchor(path, 53, Side.Right), CommentAnchor(path, 56, Side.Right), "x").error(),
            )
        }

        @Test
        fun `stale drafts cannot be added to or submitted`() = runTest {
            val backend = testBackend()
            val pending = backend.pendingReview(authorFilter).orFail()
            assertTrue(pending.stale)
            val submit = backend.submitReview(authorFilter, ReviewEvent.Comment, "x", includeDrafts = true)
            assertInstanceOf(BackendError.DraftsStale::class.java, submit.error())
            assertTrue(backend.discardDrafts(authorFilter).orFail().drafts.isEmpty())
            assertFalse(backend.pendingReview(authorFilter).orFail().stale)
        }

        @Test
        fun `submitting turns drafts into threads and clears them`() = runTest {
            val backend = testBackend()
            backend.submitReview(diffOverview, ReviewEvent.Comment, "Two nits", includeDrafts = true).orFail()
            val detail = backend.pullDetail(diffOverview).orFail()
            assertEquals(3, detail.threads.size)
            assertTrue(detail.pendingReview.drafts.isEmpty())
            val review = detail.timeline.last().kind as TimelineKind.Review
            assertEquals(2, review.threadIds.size)
        }

        @Test
        fun `a review without drafts leaves them pending`() = runTest {
            val backend = testBackend()
            backend.submitReview(diffOverview, ReviewEvent.Comment, "Summary only", includeDrafts = false).orFail()
            assertEquals(2, backend.pendingReview(diffOverview).orFail().drafts.size)
        }

        @Test
        fun `an empty comment review is refused`() = runTest {
            val backend = testBackend()
            backend.discardDrafts(diffOverview)
            assertInstanceOf(BackendError.InvalidInput::class.java, backend.submitReview(diffOverview, ReviewEvent.Comment, " ", true).error())
        }

        @Test
        fun `you cannot approve your own pull request`() = runTest {
            assertInstanceOf(BackendError.GitHubApi::class.java, testBackend().submitReview(authorFilter, ReviewEvent.Approve, "", false).error())
        }
    }

    @Nested
    inner class Desktop {
        @Test
        fun `a pairing link parses into a preview`() {
            val preview = testBackend().parsePairingLink(
                "rostrum://pair?name=nymph-desk&hosts=192.168.1.24,nymph-desk.local&port=8485&fp=4f2a91c07e3bd518&code=WDJB-MJHT",
            ).orFail()
            assertEquals("nymph-desk", preview.machine)
            assertEquals(listOf("192.168.1.24", "nymph-desk.local"), preview.hosts)
            assertEquals("4F2A · 91C0 · 7E3B", preview.fingerprintShort)
            assertEquals("WDJB-MJHT", preview.code)
        }

        @Test
        fun `other links are refused`() {
            val backend = testBackend()
            assertInstanceOf(Outcome.Err::class.java, backend.parsePairingLink("https://example.com/pair?code=x"))
            assertInstanceOf(Outcome.Err::class.java, backend.parsePairingLink("rostrum://pair?name=x"))
        }

        @Test
        fun `manual pairing checks the code and the fingerprint`() = runTest {
            val backend = testBackend(signedIn = false, paired = false)
            val probe = backend.probeDesktop("10.0.0.5", 8485).orFail()
            assertEquals(
                RemoteErrorCode.PairingCodeInvalid,
                (backend.pairManual("10.0.0.5", 8485, probe.fingerprint, "12", "Pixel").error() as BackendError.RemoteApi).code,
            )
            assertInstanceOf(BackendError.CertificateMismatch::class.java, backend.pairManual("10.0.0.5", 8485, "x", "WDJBMJHT", "Pixel").error())
            val result = backend.pairManual("10.0.0.5", 8485, probe.fingerprint, "wdjb-mjht", "Pixel").orFail()
            assertNotNull(result.github)
        }

        @Test
        fun `desktop calls need a pairing`() = runTest {
            assertEquals(BackendError.NotPaired, testBackend(paired = false).machineInfo().error())
        }

        @Test
        fun `the handed-off rebase shows as a session and aborts`() = runTest {
            val backend = testBackend()
            val session = backend.handoffs().orFail().single()
            assertEquals(diffOverview, session.pr)
            assertEquals("tmux attach -t =rostrum-RhizoNymph-rostrum-10", session.attachCommand)
            backend.abortLocal(diffOverview).orFail()
            assertTrue(backend.handoffs().orFail().isEmpty())
            val branch = (backend.localStatus(diffOverview).orFail() as LocalStatus.CheckedOut).branch
            assertNull(branch.inProgress)
        }

        @Test
        fun `a dirty worktree refuses unless stashing`() = runTest {
            val backend = testBackend()
            assertInstanceOf(JobOutcome.Refused::class.java, backend.runLocalJob(authorFilter, LocalOp.RebaseBase, false).orFail().outcome)
            assertEquals(JobOutcome.Completed, backend.runLocalJob(authorFilter, LocalOp.RebaseBase, true).orFail().outcome)
        }

        @Test
        fun `sync all finishes one entry per poll`() = runTest {
            val backend = testBackend()
            val run = backend.startSyncAll(SyncAllOp.MergeBase, autostash = false).orFail()
            assertTrue(run.running)
            assertInstanceOf(BackendError.RemoteApi::class.java, backend.startSyncAll(SyncAllOp.Pull, false).error())
            var status = run
            repeat(run.entries.size) { status = backend.syncAllStatus().orFail()!! }
            assertFalse(status.running)
            assertEquals(run.entries.size, status.summary.done)
        }
    }

    @Test
    fun `the first notification check is a baseline, the second reports`() = runTest {
        val backend = testBackend()
        assertTrue(backend.checkNotifications().orFail().isEmpty())
        val events = backend.checkNotifications().orFail()
        assertEquals(NotificationKind.ReviewRequested, events.single().kind)
        assertTrue(backend.checkNotifications().orFail().isEmpty())
    }

    @Test
    fun `failNext fails exactly once`() = runTest {
        val backend = testBackend()
        backend.failNext(FakeCall.CachedFeed, BackendError.Network("offline"))
        assertEquals(BackendError.Network("offline"), backend.cachedFeed().error())
        backend.cachedFeed().orFail()
    }
}
