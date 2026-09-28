package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DesktopGitHubToken
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.DraftAnchor
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.FileDiff
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.FilesOverview
import io.github.rhizonymph.rostrum.data.model.GitHubStatus
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.JobResult
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.PairingResult
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.ReviewDecision
import io.github.rhizonymph.rostrum.data.model.ReviewDraft
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.data.model.ReviewState
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncRun
import io.github.rhizonymph.rostrum.data.model.ThreadCommentView
import io.github.rhizonymph.rostrum.data.model.TimelineEntry
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.data.model.UserRef
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.time.Clock
import java.time.Duration

/**
 * An in-memory [RostrumBackend] with the mockups' sample data. Every action
 * really changes its state, so the app runs end to end on it, previews render
 * realistic screens, and ViewModel tests drive it directly.
 *
 * @param latency simulated round-trip time; zero in tests.
 * @param signedIn start with a GitHub token already handed in.
 * @param paired start with nymph-desk set as the session's desktop.
 */
class FakeRostrumBackend(
    private val clock: Clock = Clock.systemUTC(),
    private val latency: Duration = Duration.ZERO,
    signedIn: Boolean = false,
    paired: Boolean = false,
) : RostrumBackend {
    private val mutex = Mutex()
    private val started = clock.instant()
    private val failures = mutableMapOf<FakeCall, BackendError>()

    private var token: String? = if (signedIn) SAMPLE_TOKEN else null
    private var tokenVerified = signedIn

    private val repos = SamplePulls.repos.toMutableList()
    private var refreshIntervalSecs = 60L
    private var prsPerRepo = 30
    private var notifyNew = true
    private var notifyReviews = true
    private var autostash = false
    private var preferences = FeedPreferences(
        hideDrafts = false,
        hideEmptyRepos = true,
        authors = listOf("rhizonymph", "ada-lin"),
        includeInvolved = true,
    )
    private var query = ""
    private val collapsed = mutableSetOf(SamplePulls.RUST)
    private val loads = repos.associateWithTo(mutableMapOf<String, RepoLoad>()) {
        RepoLoad.Loaded(started.minus(Duration.ofMinutes(5)))
    }
    private val pulls = SamplePulls.pulls(started).associateByTo(LinkedHashMap()) { it.ref }
    private val labels = repos.associateWithTo(mutableMapOf()) { SamplePulls.labels(it) }
    private val conversations = mutableMapOf<PrRef, FakeConversation>()
    private val drafts = mutableMapOf<PrRef, MutableList<ReviewDraft>>()
    private val draftedAgainst = mutableMapOf<PrRef, String>()
    private var nextId = 100L
    private var revision = 0L

    private var notificationChecks = 0
    private val desktop = FakeDesktop(
        clock = clock,
        started = started,
        paired = paired,
        pulls = pulls,
        onFeedChanged = { emitFeed() },
        adoptTokenIfSignedOut = { if (token == null) token = it },
        replaceToken = {
            token = it
            tokenVerified = false
        },
    )

    private val updates = MutableSharedFlow<FeedSnapshot>(extraBufferCapacity = 64)
    override val feedUpdates: Flow<FeedSnapshot> = updates.asSharedFlow()

    init {
        val diffOverview = PrRef(SamplePulls.ROSTRUM, 10)
        drafts[diffOverview] = mutableListOf(
            draft(CommentAnchor("crates/rostrum-diff/src/overview.rs", 60, Side.Right), null,
                "A test with a rename-only diff would pin the sliver behaviour."),
            draft(CommentAnchor("crates/rostrum/src/detail/files.rs", 271, Side.Right), null,
                "Nit: the toggle bar repeats the tab bar's segment styling."),
        )
        draftedAgainst[diffOverview] = SamplePulls.DIFF_OVERVIEW_SHA
        val authorFilter = PrRef(SamplePulls.ROSTRUM, 9)
        drafts[authorFilter] = mutableListOf(
            draft(CommentAnchor("src/lib.rs", 21, Side.Right), null, "Should the roster cap be configurable?"),
        )
        draftedAgainst[authorFilter] = "a41c9e05d3b2a1f0e9d8c7b6a5f4e3d2c1b0a9f8"
    }

    /** Make the next call of [call] fail with [error] (once). */
    fun failNext(call: FakeCall, error: BackendError) {
        synchronized(failures) { failures[call] = error }
    }

    private suspend fun <T> call(op: FakeCall, block: () -> Outcome<T>): Outcome<T> {
        if (!latency.isZero) delay(latency.toMillis())
        val injected = synchronized(failures) { failures.remove(op) }
        if (injected != null) return Outcome.Err(injected)
        return mutex.withLock { block() }
    }

    private val viewerLogin: String? get() = if (token != null) SamplePulls.VIEWER else null

    private fun <T> signedIn(block: () -> Outcome<T>): Outcome<T> =
        if (token == null) Outcome.Err(BackendError.NotSignedIn) else block()

    private fun <T> withPull(pr: PrRef, block: (FakePull) -> Outcome<T>): Outcome<T> =
        signedIn { pulls[pr]?.let(block) ?: Outcome.Err(BackendError.UnknownPullRequest(pr.repo, pr.number)) }

    private fun labelsOf(repo: String): Map<String, LabelView> = labels[repo].orEmpty().associateBy { it.name }

    // --- session ---------------------------------------------------------------

    override suspend fun setGitHubToken(token: String?, host: String): Outcome<GitHubStatus> = call(FakeCall.SetGitHubToken) {
        this.token = token?.takeIf { it.isNotBlank() }?.trim()
        tokenVerified = false
        Outcome.Ok(if (this.token == null) GitHubStatus.NoToken else GitHubStatus.Unverified)
    }

    override suspend fun githubStatus(): GitHubStatus = mutex.withLock {
        val current = token
        when {
            current == null -> GitHubStatus.NoToken
            !looksLikeToken(current) -> GitHubStatus.Invalid("Bad credentials")
            tokenVerified -> GitHubStatus.Verified(UserRef(SamplePulls.VIEWER))
            else -> GitHubStatus.Unverified
        }
    }

    override suspend fun viewer(): Outcome<UserRef> = call(FakeCall.Viewer) {
        val current = token
        when {
            current == null -> Outcome.Err(BackendError.NotSignedIn)
            !looksLikeToken(current) -> Outcome.Err(BackendError.GitHubAuthFailed("Bad credentials"))
            else -> {
                tokenVerified = true
                Outcome.Ok(UserRef(SamplePulls.VIEWER))
            }
        }
    }

    override suspend fun warnings(): List<String> = emptyList()

    // --- settings ----------------------------------------------------------------

    private fun currentSettings() = Settings(
        repos = repos.toList(),
        refreshIntervalSecs = refreshIntervalSecs,
        prsPerRepo = prsPerRepo,
        notifyNewPullRequests = notifyNew,
        notifyReviewRequests = notifyReviews,
        autostash = autostash,
        feed = preferences,
    )

    override suspend fun settings(): Outcome<Settings> = call(FakeCall.Settings) { Outcome.Ok(currentSettings()) }

    override suspend fun addRepo(input: String): Outcome<String> = call(FakeCall.AddRepo) {
        when (val parsed = normalizeRepo(input)) {
            null -> Outcome.Err(BackendError.InvalidRepo(input.trim(), "expected owner/name or a GitHub URL"))
            else -> {
                val existing = repos.firstOrNull { it.equals(parsed, ignoreCase = true) }
                if (existing != null) {
                    Outcome.Err(BackendError.DuplicateRepo(existing))
                } else {
                    repos += parsed
                    loads[parsed] = RepoLoad.Idle
                    labels[parsed] = emptyList()
                    emitFeed()
                    Outcome.Ok(parsed)
                }
            }
        }
    }

    override suspend fun removeRepo(repo: String): Outcome<Boolean> = call(FakeCall.RemoveRepo) {
        val removed = repos.removeAll { it.equals(repo, ignoreCase = true) }
        if (removed) emitFeed()
        Outcome.Ok(removed)
    }

    override suspend fun setRefreshInterval(seconds: Long): Outcome<Settings> = call(FakeCall.SetRefreshInterval) {
        refreshIntervalSecs = seconds.coerceIn(10, 3600)
        Outcome.Ok(currentSettings())
    }

    override suspend fun setPrsPerRepo(count: Int): Outcome<Settings> = call(FakeCall.SetPrsPerRepo) {
        prsPerRepo = count.coerceIn(1, 100)
        Outcome.Ok(currentSettings())
    }

    override suspend fun setNotifications(newPullRequests: Boolean, reviewRequests: Boolean): Outcome<Settings> =
        call(FakeCall.SetNotifications) {
            notifyNew = newPullRequests
            notifyReviews = reviewRequests
            Outcome.Ok(currentSettings())
        }

    override suspend fun setAutostash(autostash: Boolean): Outcome<Settings> = call(FakeCall.SetAutostash) {
        this.autostash = autostash
        Outcome.Ok(currentSettings())
    }

    // --- feed --------------------------------------------------------------------

    private fun snapshot(): FeedSnapshot {
        val viewer = viewerLogin
        val candidates = pulls.values
            .filter { it.state == PullState.Open && it.repo in repos }
            .groupBy { it.repo }
            .mapValues { (repo, list) ->
                list.map { FakeFeedAssembler.Candidate(it.summary(viewer, labelsOf(repo)), it.involved) }
            }
        return FakeFeedAssembler.assemble(
            revision = revision,
            repos = repos,
            candidates = candidates,
            loads = loads,
            preferences = preferences,
            query = query,
            collapsed = collapsed,
            viewer = viewer?.let { UserRef(it) },
        )
    }

    private fun emitFeed(): Outcome<FeedSnapshot> {
        revision++
        val snapshot = snapshot()
        updates.tryEmit(snapshot)
        return Outcome.Ok(snapshot)
    }

    override suspend fun cachedFeed(): Outcome<FeedSnapshot> = call(FakeCall.CachedFeed) { Outcome.Ok(snapshot()) }

    override suspend fun refreshFeed(): Outcome<FeedSnapshot> = call(FakeCall.RefreshFeed) {
        signedIn {
            val now = clock.instant()
            repos.forEach { loads[it] = RepoLoad.Loaded(now) }
            emitFeed()
        }
    }

    override suspend fun refreshRepo(repo: String): Outcome<FeedSnapshot> = call(FakeCall.RefreshRepo) {
        signedIn {
            if (repos.none { it == repo }) {
                Outcome.Err(BackendError.InvalidInput("$repo isn't watched"))
            } else {
                loads[repo] = RepoLoad.Loaded(clock.instant())
                emitFeed()
            }
        }
    }

    override suspend fun setQuery(query: String): Outcome<FeedSnapshot> = call(FakeCall.SetQuery) {
        this.query = query
        emitFeed()
    }

    override suspend fun setFilter(preferences: FeedPreferences): Outcome<FeedSnapshot> = call(FakeCall.SetFilter) {
        this.preferences = preferences.copy(authors = preferences.authors.map { it.lowercase() }.distinct())
        emitFeed()
    }

    override suspend fun toggleAuthor(login: String): Outcome<FeedSnapshot> = call(FakeCall.ToggleAuthor) {
        val key = login.lowercase()
        val authors = preferences.authors
        preferences = preferences.copy(authors = if (key in authors) authors - key else authors + key)
        emitFeed()
    }

    override suspend fun clearFilter(): Outcome<FeedSnapshot> = call(FakeCall.ClearFilter) {
        query = ""
        preferences = FeedPreferences.Default
        emitFeed()
    }

    override suspend fun toggleCollapsed(repo: String): Outcome<FeedSnapshot> = call(FakeCall.ToggleCollapsed) {
        if (!collapsed.remove(repo)) collapsed += repo
        emitFeed()
    }

    override suspend fun authorRoster(limit: Int?): Outcome<AuthorRoster> = call(FakeCall.AuthorRoster) {
        val viewer = viewerLogin
        val open = pulls.values.filter { it.state == PullState.Open && it.repo in repos }
            .map { it.summary(viewer, labelsOf(it.repo)) }
        Outcome.Ok(FakeFeedAssembler.roster(open, viewer, preferences.authors, limit))
    }

    // --- one pull request --------------------------------------------------------

    private fun conversation(pull: FakePull): FakeConversation = conversations.getOrPut(pull.ref) {
        if (pull.ref == PrRef(SamplePulls.ROSTRUM, 10)) {
            SampleConversation.diffOverview(pull, started)
        } else {
            SampleConversation.generic(pull, started)
        }
    }

    private fun pending(pr: PrRef): PendingReview {
        val head = pulls[pr]?.headSha.orEmpty()
        val list = drafts[pr].orEmpty()
        val against = draftedAgainst[pr]?.takeIf { list.isNotEmpty() }
        return PendingReview(
            repo = pr.repo,
            number = pr.number,
            drafts = list.toList(),
            draftedAgainst = against,
            headSha = head,
            stale = against != null && against != head,
        )
    }

    private fun detail(pull: FakePull): PullDetail {
        val conversation = conversation(pull)
        return PullDetail(
            header = pull.header(viewerLogin, labelsOf(pull.repo)),
            timeline = conversation.timeline.toList(),
            threads = conversation.threads.toList(),
            checks = conversation.checks,
            unresolvedThreads = conversation.threads.count { !it.resolved },
            pendingReview = pending(pull.ref),
        )
    }

    override suspend fun pullDetail(pr: PrRef): Outcome<PullDetail> =
        call(FakeCall.PullDetail) { withPull(pr) { Outcome.Ok(detail(it)) } }

    override suspend fun cachedPullDetail(pr: PrRef): Outcome<PullDetail?> = call(FakeCall.CachedPullDetail) {
        val pull = pulls[pr]
        Outcome.Ok(if (pull != null && pr in conversations) detail(pull) else null)
    }

    override suspend fun pullHeader(pr: PrRef): Outcome<PullHeader> = call(FakeCall.PullHeader) {
        pulls[pr]?.let { Outcome.Ok(it.header(viewerLogin, labelsOf(it.repo))) }
            ?: Outcome.Err(BackendError.UnknownPullRequest(pr.repo, pr.number))
    }

    override suspend fun repositoryLabels(repo: String): Outcome<List<LabelView>> =
        call(FakeCall.RepositoryLabels) { signedIn { Outcome.Ok(labels[repo].orEmpty()) } }

    override suspend fun addLabel(pr: PrRef, label: String): Outcome<Unit> = call(FakeCall.AddLabel) {
        withPull(pr) { pull ->
            if (label !in pull.labels) {
                pulls[pr] = pull.copy(labels = pull.labels + label)
                event(pull, TimelineEvent.Labeled(label), "added the $label label")
                emitFeed()
            }
            Outcome.Ok(Unit)
        }
    }

    override suspend fun removeLabel(pr: PrRef, label: String): Outcome<Unit> = call(FakeCall.RemoveLabel) {
        withPull(pr) { pull ->
            if (label in pull.labels) {
                pulls[pr] = pull.copy(labels = pull.labels - label)
                event(pull, TimelineEvent.Unlabeled(label), "removed the $label label")
                emitFeed()
            }
            Outcome.Ok(Unit)
        }
    }

    override suspend fun addComment(pr: PrRef, body: String): Outcome<Unit> = call(FakeCall.AddComment) {
        withPull(pr) { pull ->
            if (body.isBlank()) return@withPull Outcome.Err(BackendError.InvalidInput("A comment can't be empty"))
            conversation(pull).timeline += TimelineEntry(
                id = "comment-${nextId++}",
                author = UserRef(SamplePulls.VIEWER),
                createdAt = clock.instant(),
                kind = TimelineKind.Comment(FakeMarkdown.parse(body), body),
            )
            pulls[pr] = pull.copy(comments = pull.comments + 1, updatedAt = clock.instant())
            emitFeed()
            Outcome.Ok(Unit)
        }
    }

    override fun renderMarkdown(source: String): List<MdBlock> = FakeMarkdown.parse(source)

    override suspend fun replyToThread(pr: PrRef, threadId: String, body: String): Outcome<Unit> =
        call(FakeCall.ReplyToThread) {
            withPull(pr) { pull ->
                if (body.isBlank()) return@withPull Outcome.Err(BackendError.InvalidInput("A reply can't be empty"))
                val threads = conversation(pull).threads
                val index = threads.indexOfFirst { it.id == threadId }
                if (index < 0) return@withPull Outcome.Err(BackendError.InvalidInput("That thread no longer exists"))
                val thread = threads[index]
                threads[index] = thread.copy(
                    comments = thread.comments + ThreadCommentView(
                        id = "c-${nextId++}",
                        author = UserRef(SamplePulls.VIEWER),
                        createdAt = clock.instant(),
                        body = FakeMarkdown.parse(body),
                        source = body,
                    ),
                )
                Outcome.Ok(Unit)
            }
        }

    override suspend fun merge(
        pr: PrRef,
        method: MergeMethod,
        commitTitle: String?,
        commitMessage: String?,
        expectedHeadSha: String,
    ): Outcome<Unit> = call(FakeCall.Merge) {
        withPull(pr) { pull ->
            val verdict = pull.verdict()
            when {
                pull.state != PullState.Open -> Outcome.Err(BackendError.GitHubApi(405, "Pull request is not open"))
                expectedHeadSha != pull.headSha ->
                    Outcome.Err(BackendError.MergeBlocked("Head branch was modified. Review and try the merge again."))
                verdict.blocksMerge -> Outcome.Err(BackendError.MergeBlocked(verdict.sentence))
                else -> {
                    pulls[pr] = pull.copy(state = PullState.Merged)
                    event(pull, TimelineEvent.Merged, "merged commit ${pull.headSha.take(7)} into ${pull.baseRef}")
                    emitFeed()
                    Outcome.Ok(Unit)
                }
            }
        }
    }

    override suspend fun closePullRequest(pr: PrRef): Outcome<Unit> = call(FakeCall.ClosePullRequest) {
        withPull(pr) { pull ->
            if (pull.state != PullState.Open) return@withPull Outcome.Err(BackendError.GitHubApi(422, "Pull request is not open"))
            pulls[pr] = pull.copy(state = PullState.Closed)
            event(pull, TimelineEvent.Closed, "closed this")
            emitFeed()
            Outcome.Ok(Unit)
        }
    }

    override suspend fun reopenPullRequest(pr: PrRef): Outcome<Unit> = call(FakeCall.ReopenPullRequest) {
        withPull(pr) { pull ->
            if (pull.state != PullState.Closed) return@withPull Outcome.Err(BackendError.GitHubApi(422, "Pull request is not closed"))
            pulls[pr] = pull.copy(state = PullState.Open)
            event(pull, TimelineEvent.Reopened, "reopened this")
            emitFeed()
            Outcome.Ok(Unit)
        }
    }

    override suspend fun setDraft(pr: PrRef, draft: Boolean): Outcome<Unit> = call(FakeCall.SetDraft) {
        withPull(pr) { pull ->
            pulls[pr] = pull.copy(
                isDraft = draft,
                mergeStatus = if (draft) MergeStatus.Draft else MergeStatus.Blocked,
                reviewDecision = if (draft) null else ReviewDecision.ReviewRequired,
            )
            if (draft) {
                event(pull, TimelineEvent.ConvertedToDraft, "marked this pull request as draft")
            } else {
                event(pull, TimelineEvent.ReadyForReview, "marked this pull request as ready for review")
            }
            emitFeed()
            Outcome.Ok(Unit)
        }
    }

    override suspend fun updateBranch(pr: PrRef, method: BranchUpdateMethod, expectedHeadOid: String): Outcome<Unit> =
        call(FakeCall.UpdateBranch) {
            withPull(pr) { pull ->
                when {
                    expectedHeadOid != pull.headSha -> Outcome.Err(BackendError.GitHubApi(409, "The head moved; refresh and try again"))
                    (pull.behind ?: 0) == 0 -> Outcome.Err(BackendError.GitHubApi(422, "There are no new commits on the base branch."))
                    else -> {
                        val newHead = "%040x".format(pull.headSha.hashCode().toLong() and 0xffffffffL xor nextId++)
                        pulls[pr] = pull.copy(
                            behind = 0,
                            headSha = newHead,
                            mergeStatus = if (pull.mergeStatus == MergeStatus.Behind) MergeStatus.Ready else pull.mergeStatus,
                            updatedAt = clock.instant(),
                        )
                        when (method) {
                            BranchUpdateMethod.Merge ->
                                event(pull, TimelineEvent.Pushed(listOf(newHead.take(7))), "merged ${pull.baseRef} into ${pull.headRef}")
                            BranchUpdateMethod.Rebase ->
                                event(pull, TimelineEvent.ForcePushed, "rebased ${pull.headRef} onto ${pull.baseRef}")
                        }
                        emitFeed()
                        Outcome.Ok(Unit)
                    }
                }
            }
        }

    private fun event(pull: FakePull, event: TimelineEvent, text: String) {
        conversation(pull).timeline += TimelineEntry(
            id = "event-${nextId++}",
            author = UserRef(SamplePulls.VIEWER),
            createdAt = clock.instant(),
            kind = TimelineKind.Event(event, text),
        )
    }

    // --- files and diff ------------------------------------------------------------

    override suspend fun filesOverview(pr: PrRef): Outcome<FilesOverview> = call(FakeCall.FilesOverview) {
        withPull(pr) { pull ->
            val conversation = conversation(pull)
            Outcome.Ok(FakeDiffs.overview(pull.headSha, conversation.files, conversation.threads, drafts[pr].orEmpty()))
        }
    }

    override suspend fun fileDiff(pr: PrRef, fileIndex: Int): Outcome<FileDiff> = call(FakeCall.FileDiff) {
        withPull(pr) { pull ->
            val conversation = conversation(pull)
            val file = conversation.files.getOrNull(fileIndex)
                ?: return@withPull Outcome.Err(BackendError.InvalidInput("No file #$fileIndex in this pull request"))
            val fileDrafts = drafts[pr].orEmpty()
            Outcome.Ok(
                FileDiff(
                    file = FakeDiffs.changedFile(
                        fileIndex, file,
                        threads = conversation.threads.count { it.path == file.path },
                        drafts = fileDrafts.count { it.anchor.path == file.path },
                    ),
                    headSha = pull.headSha,
                    body = FileDiffBody.Rows(FakeDiffs.rows(file, conversation.threads, fileDrafts)),
                ),
            )
        }
    }

    // --- pending review ------------------------------------------------------------

    private fun draft(anchor: CommentAnchor, rangeStart: CommentAnchor?, body: String): ReviewDraft {
        val start = rangeStart?.line
        return ReviewDraft(
            id = nextId++,
            anchor = DraftAnchor(anchor.path, anchor.line, anchor.side, start),
            body = body,
            location = if (start == null) "${anchor.path}:${anchor.line}" else "${anchor.path} lines $start–${anchor.line}",
        )
    }

    override suspend fun pendingReview(pr: PrRef): Outcome<PendingReview> =
        call(FakeCall.PendingReview) { withPull(pr) { Outcome.Ok(pending(pr)) } }

    override suspend fun addDraft(
        pr: PrRef,
        anchor: CommentAnchor,
        rangeStart: CommentAnchor?,
        body: String,
    ): Outcome<PendingReview> = call(FakeCall.AddDraft) {
        withPull(pr) { pull ->
            val current = pending(pr)
            val file = conversation(pull).files.firstOrNull { it.path == anchor.path }
            val anchors = file?.let(FakeDiffs::anchors).orEmpty()
            when {
                current.stale -> Outcome.Err(BackendError.DraftsStale(current.draftedAgainst.orEmpty(), current.headSha))
                body.isBlank() -> Outcome.Err(BackendError.InvalidInput("A comment can't be empty"))
                anchor !in anchors -> Outcome.Err(BackendError.InvalidInput("Line ${anchor.line} isn't part of this diff"))
                rangeStart != null && (rangeStart !in anchors || rangeStart.path != anchor.path ||
                    rangeStart.side != anchor.side || rangeStart.line >= anchor.line) ->
                    Outcome.Err(BackendError.InvalidInput("A range must run down one side of one file"))
                else -> {
                    drafts.getOrPut(pr) { mutableListOf() } += draft(anchor, rangeStart, body.trim())
                    draftedAgainst.putIfAbsent(pr, pull.headSha)
                    Outcome.Ok(pending(pr))
                }
            }
        }
    }

    override suspend fun editDraft(pr: PrRef, draftId: Long, body: String): Outcome<PendingReview> = call(FakeCall.EditDraft) {
        withPull(pr) {
            val list = drafts[pr]
            val index = list?.indexOfFirst { it.id == draftId } ?: -1
            when {
                body.isBlank() -> Outcome.Err(BackendError.InvalidInput("A comment can't be empty"))
                list == null || index < 0 -> Outcome.Err(BackendError.InvalidInput("That draft no longer exists"))
                else -> {
                    list[index] = list[index].copy(body = body.trim())
                    Outcome.Ok(pending(pr))
                }
            }
        }
    }

    override suspend fun removeDraft(pr: PrRef, draftId: Long): Outcome<PendingReview> = call(FakeCall.RemoveDraft) {
        withPull(pr) {
            drafts[pr]?.removeAll { it.id == draftId }
            if (drafts[pr].isNullOrEmpty()) draftedAgainst.remove(pr)
            Outcome.Ok(pending(pr))
        }
    }

    override suspend fun discardDrafts(pr: PrRef): Outcome<PendingReview> = call(FakeCall.DiscardDrafts) {
        withPull(pr) {
            drafts.remove(pr)
            draftedAgainst.remove(pr)
            Outcome.Ok(pending(pr))
        }
    }

    override suspend fun submitReview(
        pr: PrRef,
        event: ReviewEvent,
        body: String,
        includeDrafts: Boolean,
    ): Outcome<Unit> = call(FakeCall.SubmitReview) {
        withPull(pr) { pull ->
            val current = pending(pr)
            val inline = if (includeDrafts) current.drafts else emptyList()
            when {
                inline.isNotEmpty() && current.stale ->
                    Outcome.Err(BackendError.DraftsStale(current.draftedAgainst.orEmpty(), current.headSha))
                event != ReviewEvent.Approve && body.isBlank() && inline.isEmpty() ->
                    Outcome.Err(BackendError.InvalidInput("Add a summary or inline comments to submit this review"))
                event != ReviewEvent.Comment && pull.author.equals(viewerLogin, ignoreCase = true) ->
                    Outcome.Err(BackendError.GitHubApi(422, "You can't approve or request changes on your own pull request"))
                else -> {
                    applyReview(pull, event, body, inline)
                    Outcome.Ok(Unit)
                }
            }
        }
    }

    private fun applyReview(pull: FakePull, event: ReviewEvent, body: String, inline: List<ReviewDraft>) {
        val conversation = conversation(pull)
        val now = clock.instant()
        val me = UserRef(SamplePulls.VIEWER)
        val newThreads = inline.map { draft ->
            ReviewThreadView(
                id = "thread-${nextId++}",
                path = draft.anchor.path,
                line = draft.anchor.line,
                originalLine = draft.anchor.line,
                side = draft.anchor.side,
                resolved = false,
                outdated = false,
                location = draft.location,
                comments = listOf(ThreadCommentView("c-${nextId++}", me, now, FakeMarkdown.parse(draft.body), draft.body)),
                canReply = true,
            )
        }
        conversation.threads += newThreads
        val (state, chip) = when (event) {
            ReviewEvent.Comment -> ReviewState.Commented to Chip("reviewed", ColorRole.Neutral)
            ReviewEvent.Approve -> ReviewState.Approved to Chip("approved", ColorRole.Success)
            ReviewEvent.RequestChanges -> ReviewState.ChangesRequested to Chip("requested changes", ColorRole.Danger)
        }
        conversation.timeline += TimelineEntry(
            id = "review-${nextId++}",
            author = me,
            createdAt = now,
            kind = TimelineKind.Review(state, chip, FakeMarkdown.parse(body), body, newThreads.map { it.id }),
        )
        val reviewers = pull.reviewers.filterNot { it.equals(SamplePulls.VIEWER, ignoreCase = true) }
        pulls[pull.ref] = when (event) {
            ReviewEvent.Comment -> pull.copy(reviewers = reviewers)
            ReviewEvent.Approve -> pull.copy(
                reviewers = reviewers,
                reviewDecision = ReviewDecision.Approved,
                mergeStatus = if (pull.mergeStatus == MergeStatus.Blocked) MergeStatus.Ready else pull.mergeStatus,
            )
            ReviewEvent.RequestChanges -> pull.copy(
                reviewers = reviewers,
                reviewDecision = ReviewDecision.ChangesRequested,
                mergeStatus = if (pull.mergeStatus == MergeStatus.Ready) MergeStatus.Blocked else pull.mergeStatus,
            )
        }
        if (inline.isNotEmpty()) {
            drafts.remove(pull.ref)
            draftedAgainst.remove(pull.ref)
        }
        emitFeed()
    }

    // --- desktop (see FakeDesktop) ------------------------------------------------

    override fun parsePairingLink(uri: String): Outcome<PairingPreview> = desktop.parsePairingLink(uri)

    override suspend fun pairWithLink(uri: String, deviceName: String): Outcome<PairingResult> =
        call(FakeCall.PairWithLink) { desktop.pairWithLink(uri, deviceName) }

    override suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> =
        call(FakeCall.ProbeDesktop) { desktop.probeDesktop(host, port) }

    override suspend fun pairManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<PairingResult> = call(FakeCall.PairManual) { desktop.pairManual(host, port, fingerprint, code, deviceName) }

    override suspend fun setRemote(endpoint: String, deviceToken: String): Outcome<RemoteStatus> =
        call(FakeCall.SetRemote) { desktop.setRemote(endpoint, deviceToken) }

    override suspend fun clearRemote(): Outcome<Unit> = call(FakeCall.ClearRemote) { desktop.clearRemote() }

    override suspend fun remoteStatus(): Outcome<RemoteStatus> = call(FakeCall.RemoteStatus) { desktop.remoteStatus() }

    override suspend fun machineInfo(): Outcome<MachineInfo> = call(FakeCall.MachineInfo) { desktop.machineInfo(autostash) }

    override suspend fun localStatus(pr: PrRef): Outcome<LocalStatus> = call(FakeCall.LocalStatus) { desktop.localStatus(pr) }

    override suspend fun runLocalJob(pr: PrRef, op: LocalOp, autostash: Boolean): Outcome<JobResult> =
        call(FakeCall.RunLocalJob) { desktop.runLocalJob(pr, op, autostash) }

    override suspend fun abortLocal(pr: PrRef): Outcome<Unit> = call(FakeCall.AbortLocal) { desktop.abortLocal(pr) }

    override suspend fun startSyncAll(op: SyncAllOp, autostash: Boolean): Outcome<SyncRun> =
        call(FakeCall.StartSyncAll) { desktop.startSyncAll(op, autostash) }

    override suspend fun syncAllStatus(): Outcome<SyncRun?> = call(FakeCall.SyncAllStatus) { desktop.syncAllStatus() }

    override suspend fun handoffs(): Outcome<List<HandoffSession>> = call(FakeCall.Handoffs) { desktop.handoffs() }

    override suspend fun refreshGitHubTokenFromDesktop(): Outcome<DesktopGitHubToken> =
        call(FakeCall.RefreshGitHubTokenFromDesktop) { desktop.refreshGitHubTokenFromDesktop() }

    override suspend fun unpair(): Outcome<Unit> = call(FakeCall.Unpair) { desktop.unpair() }

    // --- notifications -------------------------------------------------------------

    override suspend fun checkNotifications(): Outcome<List<NotificationEvent>> = call(FakeCall.CheckNotifications) {
        signedIn {
            notificationChecks++
            if (notificationChecks != 2) return@signedIn Outcome.Ok(emptyList())
            val pull = FakePull(
                repo = SamplePulls.ROSTRUM, number = 12,
                title = "fix: keep the feed's scroll position across refreshes",
                author = "ada-lin", createdAt = clock.instant(), updatedAt = clock.instant(),
                checks = io.github.rhizonymph.rostrum.data.model.CheckState.Pending,
                mergeStatus = MergeStatus.Blocked, behind = 0,
                additions = 34, deletions = 6, changedFiles = 2,
                reviewers = listOf(SamplePulls.VIEWER), headRef = "fix/feed-scroll",
                headSha = "c0ffee0123456789abcdef0123456789abcdef01",
            )
            pulls[pull.ref] = pull
            emitFeed()
            val kind = when {
                notifyReviews -> NotificationKind.ReviewRequested
                notifyNew -> NotificationKind.NewPullRequest
                else -> return@signedIn Outcome.Ok(emptyList())
            }
            Outcome.Ok(listOf(NotificationEvent(kind, pull.repo, pull.number, pull.title, pull.author, "https://github.com/${pull.repo}/pull/12")))
        }
    }

    override suspend fun markNotificationsSeen(): Outcome<Unit> = call(FakeCall.MarkNotificationsSeen) { Outcome.Ok(Unit) }

    companion object {
        const val SAMPLE_TOKEN = "ghp_sampleTokenForPreviews0123456789"

        private val tokenPrefixes = listOf("ghp_", "github_pat_", "gho_", "ghu_", "ghs_")

        /** GitHub tokens start with a known prefix; anything else is rejected as a bad credential. */
        fun looksLikeToken(token: String): Boolean = tokenPrefixes.any { token.startsWith(it) } && token.length >= 20

        /** `owner/name` from `owner/name`, `github.com/owner/name` or a full URL; `null` if neither. */
        fun normalizeRepo(input: String): String? {
            val trimmed = input.trim()
                .removePrefix("https://").removePrefix("http://").removePrefix("www.")
                .removePrefix("github.com/")
                .removeSuffix("/").removeSuffix(".git")
            val parts = trimmed.split('/')
            if (parts.size < 2) return null
            val (owner, name) = parts
            val ok = Regex("^[A-Za-z0-9-]+$").matches(owner) && Regex("^[A-Za-z0-9._-]+$").matches(name)
            return if (ok && (parts.size == 2 || parts[2] in setOf("pulls", "pull", "tree", "issues"))) "$owner/$name" else null
        }

        /** `4F2A · 91C0 · 7E3B` from a hex fingerprint (any separators). */
        fun shortFingerprint(fingerprint: String): String =
            fingerprint.substringAfter(':').filter { it.isLetterOrDigit() }.uppercase().take(12).chunked(4).joinToString(" · ")
    }
}
