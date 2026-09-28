package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DiffRow
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.GitHubStatus
import io.github.rhizonymph.rostrum.data.model.JobOutcome
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.LineKind
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncEntryState
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import uniffi.rostrum_ffi.InternalException
import uniffi.rostrum_ffi.RostrumException
import java.time.Instant
import uniffi.rostrum_ffi.BaseDivergence as FBaseDivergence
import uniffi.rostrum_ffi.ChangedFile as FChangedFile
import uniffi.rostrum_ffi.CheckRunView as FCheckRunView
import uniffi.rostrum_ffi.CheckState as FCheckState
import uniffi.rostrum_ffi.Chip as FChip
import uniffi.rostrum_ffi.CodeSegment as FCodeSegment
import uniffi.rostrum_ffi.ColorRole as FColorRole
import uniffi.rostrum_ffi.CommentAnchor as FCommentAnchor
import uniffi.rostrum_ffi.DiffAvailability as FDiffAvailability
import uniffi.rostrum_ffi.DiffLineView as FDiffLineView
import uniffi.rostrum_ffi.DiffRow as FDiffRow
import uniffi.rostrum_ffi.DraftAction as FDraftAction
import uniffi.rostrum_ffi.DraftAnchor as FDraftAnchor
import uniffi.rostrum_ffi.FeedPreferences as FFeedPreferences
import uniffi.rostrum_ffi.FeedSnapshot as FFeedSnapshot
import uniffi.rostrum_ffi.FileDiff as FFileDiff
import uniffi.rostrum_ffi.FileDiffBody as FFileDiffBody
import uniffi.rostrum_ffi.FileStatus as FFileStatus
import uniffi.rostrum_ffi.GitHubStatus as FGitHubStatus
import uniffi.rostrum_ffi.HandoffSession as FHandoffSession
import uniffi.rostrum_ffi.HandoffState as FHandoffState
import uniffi.rostrum_ffi.InProgress as FInProgress
import uniffi.rostrum_ffi.InProgressKind as FInProgressKind
import uniffi.rostrum_ffi.JobOutcome as FJobOutcome
import uniffi.rostrum_ffi.JobResult as FJobResult
import uniffi.rostrum_ffi.LabelView as FLabelView
import uniffi.rostrum_ffi.LineKind as FLineKind
import uniffi.rostrum_ffi.LocalBranch as FLocalBranch
import uniffi.rostrum_ffi.LocalOp as FLocalOp
import uniffi.rostrum_ffi.LocalStatus as FLocalStatus
import uniffi.rostrum_ffi.LogField as FLogField
import uniffi.rostrum_ffi.LogLevel as FLogLevel
import uniffi.rostrum_ffi.LogRecord as FLogRecord
import uniffi.rostrum_ffi.MdBlock as FMdBlock
import uniffi.rostrum_ffi.MdBlockKind as FMdBlockKind
import uniffi.rostrum_ffi.MdSpan as FMdSpan
import uniffi.rostrum_ffi.MergeStatus as FMergeStatus
import uniffi.rostrum_ffi.MergeVerdict as FMergeVerdict
import uniffi.rostrum_ffi.PendingReview as FPendingReview
import uniffi.rostrum_ffi.PrSummary as FPrSummary
import uniffi.rostrum_ffi.PullDetail as FPullDetail
import uniffi.rostrum_ffi.PullHeader as FPullHeader
import uniffi.rostrum_ffi.PullState as FPullState
import uniffi.rostrum_ffi.RemoteErrorCode as FRemoteErrorCode
import uniffi.rostrum_ffi.RemoteStatus as FRemoteStatus
import uniffi.rostrum_ffi.RepoBody as FRepoBody
import uniffi.rostrum_ffi.RepoLoad as FRepoLoad
import uniffi.rostrum_ffi.RepoSection as FRepoSection
import uniffi.rostrum_ffi.ReviewDecision as FReviewDecision
import uniffi.rostrum_ffi.ReviewDraft as FReviewDraft
import uniffi.rostrum_ffi.ReviewState as FReviewState
import uniffi.rostrum_ffi.ReviewThreadView as FReviewThreadView
import uniffi.rostrum_ffi.Side as FSide
import uniffi.rostrum_ffi.SyncEntry as FSyncEntry
import uniffi.rostrum_ffi.SyncEntryState as FSyncEntryState
import uniffi.rostrum_ffi.SyncRun as FSyncRun
import uniffi.rostrum_ffi.SyncSummary as FSyncSummary
import uniffi.rostrum_ffi.ThreadCommentView as FThreadCommentView
import uniffi.rostrum_ffi.TimelineEntry as FTimelineEntry
import uniffi.rostrum_ffi.TimelineEvent as FTimelineEvent
import uniffi.rostrum_ffi.TimelineKind as FTimelineKind
import uniffi.rostrum_ffi.UserRef as FUserRef

/**
 * The generated → model mapping, on the JVM. Constructing generated records
 * and exceptions does not load the native library.
 */
class FfiMappingsTest {
    private val at = Instant.parse("2026-09-28T12:00:00Z")
    private val ada = FUserRef("ada-lin", null)

    @Nested
    inner class Errors {
        @Test
        fun `every core error maps to its BackendError`() {
            val cases: List<Pair<RostrumException, BackendError>> = listOf(
                RostrumException.NotSignedIn() to BackendError.NotSignedIn,
                RostrumException.GitHubAuthFailed("bad") to BackendError.GitHubAuthFailed("bad"),
                RostrumException.GitHubRateLimited(at) to BackendError.GitHubRateLimited(at),
                RostrumException.MergeBlocked("protected") to BackendError.MergeBlocked("protected"),
                RostrumException.GitHubApi(422.toUShort(), "nope") to BackendError.GitHubApi(422, "nope"),
                RostrumException.GitHubApi(null, "graphql") to BackendError.GitHubApi(null, "graphql"),
                RostrumException.Network("offline") to BackendError.Network("offline"),
                RostrumException.UnknownPullRequest("a/b", 7u) to BackendError.UnknownPullRequest("a/b", 7),
                RostrumException.DraftsStale("abc", "def") to BackendError.DraftsStale("abc", "def"),
                RostrumException.NotPaired() to BackendError.NotPaired,
                RostrumException.DeviceRevoked() to BackendError.DeviceRevoked,
                RostrumException.DesktopUnreachable("refused") to BackendError.DesktopUnreachable("refused"),
                RostrumException.CertificateMismatch("h") to BackendError.CertificateMismatch("h"),
                RostrumException.DesktopTimeout() to BackendError.DesktopTimeout,
                RostrumException.IncompatibleDesktop(2u, 1u) to BackendError.IncompatibleDesktop(2, 1),
                RostrumException.RemoteApi(FRemoteErrorCode.BUSY, "busy") to BackendError.RemoteApi(RemoteErrorCode.Busy, "busy"),
                RostrumException.RemoteProtocol("garbage") to BackendError.RemoteProtocol("garbage"),
                RostrumException.InvalidRepo("x", "not owner/name") to BackendError.InvalidRepo("x", "not owner/name"),
                RostrumException.DuplicateRepo("a/b") to BackendError.DuplicateRepo("a/b"),
                RostrumException.InvalidInput("why") to BackendError.InvalidInput("why"),
                RostrumException.Storage("disk") to BackendError.Storage("disk"),
                RostrumException.Internal("bug") to BackendError.Internal("bug"),
            )
            cases.forEach { (ffi, model) -> assertEquals(model, ffi.toBackendError(), ffi.javaClass.simpleName) }
        }

        @Test
        fun `every remote error code maps`() {
            val expected = listOf(
                RemoteErrorCode.Unauthorized, RemoteErrorCode.Forbidden, RemoteErrorCode.BadRequest,
                RemoteErrorCode.NotFound, RemoteErrorCode.PairingCodeInvalid, RemoteErrorCode.PairingCodeExpired,
                RemoteErrorCode.RateLimited, RemoteErrorCode.Busy, RemoteErrorCode.Internal,
            )
            assertEquals(expected, FRemoteErrorCode.entries.map { it.toModel() })
        }

        @Test
        fun `ffiCall turns core errors and panics into Err and passes values through`() {
            assertEquals(Outcome.Ok(3), ffiCall("ok") { 3 })
            assertEquals(Outcome.Err(BackendError.NotPaired), ffiCall<Int>("paired") { throw RostrumException.NotPaired() })
            val panic = ffiCall<Int>("panic") { throw InternalException("Rust panic") }
            assertEquals(Outcome.Err(BackendError.Internal("Rust panic")), panic)
        }
    }

    @Nested
    inner class Shared {
        @Test
        fun `enums map one to one`() {
            assertEquals(
                listOf(ColorRole.Success, ColorRole.Warning, ColorRole.Danger, ColorRole.Draft, ColorRole.Accent, ColorRole.Neutral),
                FColorRole.entries.map { it.toModel() },
            )
            assertEquals(CheckState.entries, FCheckState.entries.map { it.toModel() })
            assertEquals(MergeStatus.entries, FMergeStatus.entries.map { it.toModel() })
            assertEquals(FSide.entries, Side.entries.map { it.toFfi() })
            Side.entries.forEach { assertEquals(it, it.toFfi().toModel()) }
            assertEquals(3, FPullState.entries.map { it.toModel() }.toSet().size)
            assertEquals(5, FReviewState.entries.map { it.toModel() }.toSet().size)
            assertEquals(3, FReviewDecision.entries.map { it.toModel() }.toSet().size)
        }

        @Test
        fun `label colours keep their ARGB bits`() {
            assertEquals(LabelView("bug", 0xFFD73A4A.toInt()), FLabelView("bug", 0xFFD73A4Au).toModel())
            assertEquals(LabelView("plain", null), FLabelView("plain", null).toModel())
        }

        @Test
        fun `markdown blocks flatten with their depths`() {
            val block = FMdBlock(
                FMdBlockKind.ListItem(ordered = true, number = 3uL, checked = false),
                listOf(FMdSpan("done", bold = true, italic = false, code = false, strike = false, link = "https://x")),
                quoteDepth = 1u,
                listDepth = 2u,
            )
            val model = block.toModel()
            assertEquals(MdBlockKind.ListItem(true, 3, false), model.kind)
            assertEquals(1, model.quoteDepth)
            assertEquals(2, model.listDepth)
            assertEquals("https://x", model.spans.single().link)
            assertEquals(MdBlockKind.Heading(2), FMdBlockKind.Heading(2.toUByte()).toModel())
            val table = FMdBlockKind.TableRow(listOf(listOf(FMdSpan("a", false, false, true, false, null))), header = true).toModel()
            assertEquals(true, (table as MdBlockKind.TableRow).cells.single().single().code)
        }
    }

    @Nested
    inner class Feed {
        private val summary = FPrSummary(
            repo = "RhizoNymph/rostrum", number = 10u, title = "feat: overview", url = "u", author = ada,
            createdAt = at, updatedAt = at, isDraft = false, checks = FCheckState.SUCCESS, checksRole = FColorRole.SUCCESS,
            reviewDecision = FReviewDecision.REVIEW_REQUIRED, reviewChip = null, mergeStatus = FMergeStatus.BLOCKED,
            mergeChip = FChip("blocked", FColorRole.WARNING, "a review is required"),
            baseDivergence = FBaseDivergence(4u, 3u, "main", false, "4 commits behind main, 3 ahead"),
            behindChip = FChip("↓4 main", FColorRole.WARNING, null), labels = listOf(FLabelView("diff_review", null)),
            additions = 900u, deletions = 9u, changedFiles = 7u, commentCount = 3u, reviewRequested = true,
            isYours = false, headRef = "feat/diff-overview", baseRef = "main",
        )

        @Test
        fun `a snapshot maps sections, bodies and the pull request row`() {
            val snapshot = FFeedSnapshot(
                revision = 42uL,
                repos = listOf(
                    FRepoSection("RhizoNymph/rostrum", FRepoLoad.Loaded(at), 3u, 1u, false, FRepoBody.Pulls(listOf(summary))),
                    FRepoSection("rust-lang/rust", FRepoLoad.Failed("rate", at), 0u, 0u, false, FRepoBody.Failed("rate")),
                    FRepoSection("x/y", FRepoLoad.Idle, 0u, 0u, true, FRepoBody.Collapsed),
                ),
                hiddenEmptyRepos = 2u, totalOpen = 14u, visibleOpen = 3u, query = "q",
                preferences = FFeedPreferences(false, true, listOf("ada-lin"), true),
                filterActive = true, mergeStatesSettling = true, viewer = ada,
            )
            val model = snapshot.toModel()
            assertEquals(42L, model.revision)
            assertEquals(RepoLoad.Loaded(at), model.repos[0].load)
            assertEquals(RepoBody.Failed("rate"), model.repos[1].body)
            assertEquals(RepoBody.Collapsed, model.repos[2].body)
            val pr = (model.repos[0].body as RepoBody.Pulls).pulls.single()
            assertEquals(10, pr.number)
            assertEquals(Chip("blocked", ColorRole.Warning, "a review is required"), pr.mergeChip)
            assertEquals(4, pr.baseDivergence!!.behind)
            assertEquals(900, pr.additions)
            assertEquals(FeedPreferences(false, true, listOf("ada-lin"), true), model.preferences)
            assertEquals("ada-lin", model.viewer!!.login)
        }

        @Test
        fun `preferences round trip`() {
            val prefs = FeedPreferences(hideDrafts = true, hideEmptyRepos = false, authors = listOf("a"), includeInvolved = true)
            assertEquals(prefs, prefs.toFfi().toModel())
        }

        @Test
        fun `github status variants`() {
            assertEquals(GitHubStatus.NoToken, FGitHubStatus.NoToken.toModel())
            assertEquals(GitHubStatus.Unverified, FGitHubStatus.Unverified.toModel())
            assertEquals(GitHubStatus.Verified(io.github.rhizonymph.rostrum.data.model.UserRef("ada-lin")), FGitHubStatus.Verified(ada).toModel())
            assertEquals(GitHubStatus.Invalid("401"), FGitHubStatus.Invalid("401").toModel())
        }
    }

    @Nested
    inner class Detail {
        @Test
        fun `a detail maps header, timeline, threads and checks`() {
            val header = FPullHeader(
                repo = "a/b", number = 5u, title = "t", url = "u", state = FPullState.OPEN, isDraft = false, author = ada,
                createdAt = at, updatedAt = at, headRef = "h", baseRef = "main", headSha = "136c158",
                labels = emptyList(), assignees = emptyList(), reviewRequests = listOf(ada),
                reviewDecision = null, reviewChip = null,
                merge = FMergeVerdict(FMergeStatus.READY, "Ready", false, FColorRole.SUCCESS, null),
                divergence = null, checks = null, checksRole = FColorRole.NEUTRAL, changedFiles = 1u, additions = 2u,
                deletions = 3u, commentCount = 0u, isYours = true, reviewRequested = false,
                draftAction = FDraftAction(true, "Convert to draft"),
            )
            val thread = FReviewThreadView(
                id = "t1", path = "src/a.rs", line = 60u, originalLine = null, side = FSide.RIGHT, resolved = false,
                outdated = false, location = "src/a.rs:60",
                comments = listOf(FThreadCommentView("c1", ada, at, emptyList(), "hi")), canReply = true,
            )
            val detail = FPullDetail(
                header = header,
                timeline = listOf(
                    FTimelineEntry("e1", ada, at, FTimelineKind.Event(FTimelineEvent.Renamed("a", "b"), "renamed")),
                    FTimelineEntry("r1", ada, at, FTimelineKind.Review(FReviewState.APPROVED, FChip("approved", FColorRole.SUCCESS, null), emptyList(), "", listOf("t1"))),
                ),
                threads = listOf(thread),
                checks = listOf(FCheckRunView("ci / fmt", null, FColorRole.NEUTRAL, "skipped", null)),
                unresolvedThreads = 1u,
                pendingReview = FPendingReview("a/b", 5u, emptyList(), null, "136c158", false),
            ).toModel()
            assertEquals(MergeStatus.Ready, detail.header.merge.status)
            assertEquals(true, detail.header.draftAction.toDraft)
            assertEquals(TimelineEvent.Renamed("a", "b"), (detail.timeline[0].kind as TimelineKind.Event).event)
            assertEquals(listOf("t1"), (detail.timeline[1].kind as TimelineKind.Review).threadIds)
            assertEquals(60, detail.threads.single().line)
            assertEquals(Side.Right, detail.threads.single().side)
            assertEquals(null, detail.checks.single().state)
            assertEquals(1, detail.unresolvedThreads)
        }

        @Test
        fun `input enums map`() {
            assertEquals(3, MergeMethod.entries.map { it.toFfi() }.toSet().size)
            assertEquals(3, ReviewEvent.entries.map { it.toFfi() }.toSet().size)
            LocalOp.entries.forEach { assertEquals(it, it.toFfi().toModel()) }
            assertEquals(3, SyncAllOp.entries.map { it.toFfi() }.toSet().size)
            assertEquals(TimelineEvent.Other("committed"), FTimelineEvent.Other("committed").toModel())
        }
    }

    @Nested
    inner class Diff {
        @Test
        fun `rows keep anchors, colours and placement`() {
            val anchor = FCommentAnchor("src/a.rs", 60u, FSide.RIGHT)
            val file = FChangedFile(0u, "src/a.rs", null, FFileStatus.ADDED, 10u, 0u, FDiffAvailability.TEXT, 1u, 0u)
            val diff = FFileDiff(
                file = file,
                headSha = "abc",
                body = FFileDiffBody.Rows(
                    listOf(
                        FDiffRow.Hunk(0u, "@@ -0,0 +1,10 @@"),
                        FDiffRow.Line(
                            FDiffLineView(
                                FLineKind.ADDED, null, 60u,
                                listOf(FCodeSegment("fn", 0xFFC4A1FFu, bold = false, italic = false, emphasized = true)),
                                anchor, noNewlineAtEof = false,
                            ),
                        ),
                        FDiffRow.Draft(FReviewDraft(9uL, FDraftAnchor("src/a.rs", 60u, FSide.RIGHT, 58u), "nit", "src/a.rs lines 58–60")),
                    ),
                ),
            ).toModel()
            val rows = (diff.body as FileDiffBody.Rows).rows
            val line = (rows[1] as DiffRow.Line).line
            assertEquals(LineKind.Added, line.kind)
            assertEquals(CommentAnchor("src/a.rs", 60, Side.Right), line.anchor)
            assertEquals(0xFFC4A1FF.toInt(), line.segments.single().argb)
            assertEquals(true, line.segments.single().emphasized)
            val draft = (rows[2] as DiffRow.Draft).draft
            assertEquals(9L, draft.id)
            assertEquals(58, draft.anchor.startLine)
            assertEquals(FileDiffBody.Unavailable, FFileDiffBody.Unavailable.toModel())
        }

        @Test
        fun `anchors round trip unchanged`() {
            val anchor = CommentAnchor("src/a.rs", 12, Side.Left)
            assertEquals(anchor, anchor.toFfi().toModel())
        }
    }

    @Nested
    inner class Remote {
        @Test
        fun `local status, jobs and sync runs map`() {
            val branch = FLocalBranch(
                worktree = "~/w", branch = "feat/x", ahead = 2u, behind = 0u, fetched = true, blocker = null,
                inProgress = FInProgress(FInProgressKind.REBASE, "a rebase is in progress", abortable = true),
                handoff = FHandoffState("s", running = true, attachCommand = "tmux attach -t =s"),
            )
            val status = FLocalStatus.CheckedOut(branch).toModel() as LocalStatus.CheckedOut
            assertEquals(2, status.branch.ahead)
            assertEquals(true, status.branch.inProgress!!.abortable)
            assertEquals(LocalStatus.NotConfigured, FLocalStatus.NotConfigured.toModel())
            assertEquals(JobOutcome.HandedOff("s", "cmd"), FJobOutcome.HandedOff("s", "cmd").toModel())
            val result = FJobResult(FJobOutcome.Refused("dirty"), "refused: dirty", FChip("refused", FColorRole.WARNING, null)).toModel()
            assertEquals(JobOutcome.Refused("dirty"), result.outcome)
            val run = FSyncRun(
                id = 1uL, op = FLocalOp.REBASE_BASE, startedAt = at, finishedAt = null,
                entries = listOf(FSyncEntry("a/b", 3u, "h", FSyncEntryState.Running)),
                summary = FSyncSummary(1u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u), progressText = "0/1…",
            ).toModel()
            assertEquals(LocalOp.RebaseBase, run.op)
            assertEquals(true, run.running)
            assertEquals(SyncEntryState.Running, run.entries.single().state)
        }

        @Test
        fun `remote status and handoffs map`() {
            val paired = FRemoteStatus.Paired(listOf("10.0.0.5"), 8485.toUShort(), "4F2A · 91C0 · 7E3B", "10.0.0.5").toModel()
            assertEquals(RemoteStatus.Paired(listOf("10.0.0.5"), 8485, "4F2A · 91C0 · 7E3B", "10.0.0.5"), paired)
            assertEquals(RemoteStatus.NotPaired, FRemoteStatus.NotPaired.toModel())
            val session = FHandoffSession("s", "a/b", 3u, "h", "~/w", at, "tmux attach -t =s").toModel()
            assertEquals(3, session.pr!!.number)
            assertInstanceOf(io.github.rhizonymph.rostrum.data.model.HandoffSession::class.java, session)
        }
    }

    @Test
    fun `core log records become key=value lines`() {
        val record = FLogRecord(FLogLevel.WARN, "rostrum_ffi::feed", "refresh failed", listOf(FLogField("repo", "a/b")))
        assertEquals("event=core target=rostrum_ffi::feed msg=\"refresh failed\" repo=a/b", FfiLogSink.format(record))
        assertEquals(android.util.Log.WARN, FfiLogSink.priorityOf(FLogLevel.WARN))
    }
}
