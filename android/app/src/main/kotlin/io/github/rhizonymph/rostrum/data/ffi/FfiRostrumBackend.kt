package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.BranchTree
import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.RepoOverview
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.model.SortSettings
import io.github.rhizonymph.rostrum.data.model.TrunkSettings
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.DesktopGitHubToken
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.FileDiff
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
import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.PairingResult
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullDetail
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.ReviewEvent
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncRun
import io.github.rhizonymph.rostrum.data.model.UserRef
import io.github.rhizonymph.rostrum.data.valueOrNull
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import uniffi.rostrum_ffi.FeedObserver
import uniffi.rostrum_ffi.RostrumCore
import uniffi.rostrum_ffi.renderMarkdown as ffiRenderMarkdown
import java.io.File
import uniffi.rostrum_ffi.FeedSnapshot as FfiFeedSnapshot

/**
 * [RostrumBackend] over the generated `uniffi.rostrum_ffi` bindings. Each
 * method is one core call, its records mapped to the app's model and its
 * errors to [BackendError]. The core opens on the first call through
 * [openCore] (see [CoreHandle]); the app has one instance per profile, over
 * that profile's core from the registry.
 *
 * @param name says which core in logs.
 */
class FfiRostrumBackend(name: String, openCore: CoreOpener) : RostrumBackend {
    /** A standalone core in [dataDir], outside any registry (the host tests). */
    constructor(dataDir: File) : this(dataDir.name, CoreHandle.inDirectory(dataDir))

    private val updates = MutableSharedFlow<FeedSnapshot>(
        replay = 1,
        extraBufferCapacity = 16,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )
    override val feedUpdates: Flow<FeedSnapshot> = updates.asSharedFlow()

    /** Background feed changes, delivered by the core on its own threads, in revision order. */
    private val observer = object : FeedObserver {
        override fun feedChanged(snapshot: FfiFeedSnapshot) {
            updates.tryEmit(snapshot.toModel())
        }
    }

    private val handle = CoreHandle(name, openCore, onOpened = { core ->
        ffiCall("setFeedObserver") { core.setFeedObserver(observer) }
            .onFailure { RostrumLog.w(CORE_LOG_TAG, "feed_observer_failed", "error" to it::class.simpleName) }
    })

    private suspend inline fun <T> core(op: String, block: (RostrumCore) -> T): Outcome<T> =
        when (val opened = handle.get()) {
            is Outcome.Err -> opened
            is Outcome.Ok -> ffiCall(op) { block(opened.value) }
        }

    private inline fun <T> Outcome<T>.onFailure(action: (BackendError) -> Unit): Outcome<T> {
        if (this is Outcome.Err) action(error)
        return this
    }

    private val PrRef.n: UInt get() = number.toUInt()

    private fun invalid(reason: String) = Outcome.Err(BackendError.InvalidInput(reason))

    // --- session -------------------------------------------------------------

    override suspend fun setGitHubToken(token: String?): Outcome<GitHubStatus> =
        core("setGithubToken") { it.setGithubToken(token).toModel() }

    override suspend fun githubStatus(): GitHubStatus =
        core("githubStatus") { it.githubStatus().toModel() }.valueOrNull() ?: GitHubStatus.NoToken

    override suspend fun viewer(): Outcome<UserRef> = core("viewer") { it.viewer().toModel() }

    override suspend fun warnings(): List<String> = when (val result = core("warnings") { it.warnings() }) {
        is Outcome.Ok -> result.value
        is Outcome.Err -> listOf(result.error.describe())
    }

    // --- settings ------------------------------------------------------------

    override suspend fun settings(): Outcome<Settings> = core("settings") { it.settings().toModel() }

    override suspend fun addRepo(input: String): Outcome<String> = core("addRepo") { it.addRepo(input) }

    override suspend fun removeRepo(repo: String): Outcome<Boolean> = core("removeRepo") { it.removeRepo(repo) }

    override suspend fun setRefreshInterval(seconds: Long): Outcome<Settings> =
        core("setRefreshInterval") { it.setRefreshInterval(seconds.coerceAtLeast(0).toULong()).toModel() }

    override suspend fun setPrsPerRepo(count: Int): Outcome<Settings> =
        core("setPrsPerRepo") { it.setPrsPerRepo(count.coerceAtLeast(0).toUInt()).toModel() }

    override suspend fun setNotifications(newPullRequests: Boolean, reviewRequests: Boolean): Outcome<Settings> =
        core("setNotifications") { it.setNotifications(newPullRequests, reviewRequests).toModel() }

    override suspend fun setAutostash(autostash: Boolean): Outcome<Settings> =
        core("setAutostash") { it.setAutostash(autostash).toModel() }

    // --- feed ----------------------------------------------------------------

    override suspend fun cachedFeed(): Outcome<FeedSnapshot> = core("cachedFeed") { it.cachedFeed().toModel() }

    override suspend fun refreshFeed(): Outcome<FeedSnapshot> = core("refreshFeed") { it.refreshFeed().toModel() }

    override suspend fun refreshRepo(repo: String): Outcome<FeedSnapshot> =
        core("refreshRepo") { it.refreshRepo(repo).toModel() }

    override suspend fun setQuery(query: String): Outcome<FeedSnapshot> = core("setQuery") { it.setQuery(query).toModel() }

    override suspend fun setFilter(preferences: FeedPreferences): Outcome<FeedSnapshot> =
        core("setFilter") { it.setFilter(preferences.toFfi()).toModel() }

    override suspend fun toggleAuthor(login: String): Outcome<FeedSnapshot> =
        core("toggleAuthor") { it.toggleAuthor(login).toModel() }

    override suspend fun clearFilter(): Outcome<FeedSnapshot> = core("clearFilter") { it.clearFilter().toModel() }

    override suspend fun toggleCollapsed(repo: String): Outcome<FeedSnapshot> =
        core("toggleCollapsed") { it.toggleCollapsed(repo).toModel() }

    override suspend fun authorRoster(limit: Int?): Outcome<AuthorRoster> =
        core("authorRoster") { it.authorRoster(limit?.coerceAtLeast(0)?.toUInt()).toModel() }

    override suspend fun setFeedTab(tab: FeedTab): Outcome<FeedSnapshot> =
        core("setFeedTab") { it.setFeedTab(tab.toFfi()).toModel() }

    // --- sort ------------------------------------------------------------------

    override suspend fun sortSettings(): Outcome<SortSettings> = core("sortSettings") { it.sortSettings().toModel() }

    override suspend fun setRepoSort(key: RepoSortKey, direction: SortDirection?): Outcome<FeedSnapshot> =
        core("setRepoSort") { it.setRepoSort(key.toFfi(), direction?.toFfi()).toModel() }

    override suspend fun setItemSort(key: ItemSortKey, direction: SortDirection?): Outcome<FeedSnapshot> =
        core("setItemSort") { it.setItemSort(key.toFfi(), direction?.toFfi()).toModel() }

    // --- issues ----------------------------------------------------------------

    private val IssueRef.n: UInt get() = number.toUInt()

    override suspend fun issueDetail(issue: IssueRef): Outcome<IssueDetail> =
        core("issueDetail") { it.issueDetail(issue.repo, issue.n).toModel() }

    override suspend fun cachedIssueDetail(issue: IssueRef): Outcome<IssueDetail?> =
        core("cachedIssueDetail") { it.cachedIssueDetail(issue.repo, issue.n)?.toModel() }

    override suspend fun commentOnIssue(issue: IssueRef, body: String): Outcome<Unit> =
        core("commentOnIssue") { it.commentOnIssue(issue.repo, issue.n, body) }

    override suspend fun closeIssue(issue: IssueRef, reason: CloseIssueAs): Outcome<Unit> =
        core("closeIssue") { it.closeIssue(issue.repo, issue.n, reason.toFfi()) }

    override suspend fun reopenIssue(issue: IssueRef): Outcome<Unit> =
        core("reopenIssue") { it.reopenIssue(issue.repo, issue.n) }

    override suspend fun addIssueLabel(issue: IssueRef, label: String): Outcome<Unit> =
        core("addIssueLabel") { it.addIssueLabel(issue.repo, issue.n, label) }

    override suspend fun removeIssueLabel(issue: IssueRef, label: String): Outcome<Unit> =
        core("removeIssueLabel") { it.removeIssueLabel(issue.repo, issue.n, label) }

    override suspend fun assignableUsers(repo: String): Outcome<List<UserRef>> =
        core("assignableUsers") { core -> core.assignableUsers(repo).map { it.toModel() } }

    override suspend fun addIssueAssignee(issue: IssueRef, login: String): Outcome<Unit> =
        core("addIssueAssignee") { it.addIssueAssignee(issue.repo, issue.n, login) }

    override suspend fun removeIssueAssignee(issue: IssueRef, login: String): Outcome<Unit> =
        core("removeIssueAssignee") { it.removeIssueAssignee(issue.repo, issue.n, login) }

    override suspend fun createIssue(
        repo: String,
        title: String,
        body: String,
        labels: List<String>,
        assignees: List<String>,
    ): Outcome<Int> = core("createIssue") { it.createIssue(repo, title, body, labels, assignees).toInt() }

    // --- one repository --------------------------------------------------------

    override suspend fun repoOverview(repo: String): Outcome<RepoOverview> =
        core("repoOverview") { it.repoOverview(repo).toModel() }

    override suspend fun branchTree(repo: String): Outcome<BranchTree> =
        core("branchTree") { it.branchTree(repo).toModel() }

    override suspend fun trunks(repo: String): Outcome<TrunkSettings> = core("trunks") { it.trunks(repo).toModel() }

    override suspend fun setTrunks(repo: String, names: List<String>?): Outcome<TrunkSettings> =
        core("setTrunks") { it.setTrunks(repo, names).toModel() }

    // --- one pull request ----------------------------------------------------

    override suspend fun pullDetail(pr: PrRef): Outcome<PullDetail> =
        core("pullDetail") { it.pullDetail(pr.repo, pr.n).toModel() }

    override suspend fun cachedPullDetail(pr: PrRef): Outcome<PullDetail?> =
        core("cachedPullDetail") { it.cachedPullDetail(pr.repo, pr.n)?.toModel() }

    override suspend fun pullHeader(pr: PrRef): Outcome<PullHeader> =
        core("pullHeader") { it.pullHeader(pr.repo, pr.n).toModel() }

    override suspend fun repositoryLabels(repo: String): Outcome<List<LabelView>> =
        core("repositoryLabels") { it.repositoryLabels(repo).map { label -> label.toModel() } }

    override suspend fun addLabel(pr: PrRef, label: String): Outcome<Unit> =
        core("addLabel") { it.addLabel(pr.repo, pr.n, label) }

    override suspend fun removeLabel(pr: PrRef, label: String): Outcome<Unit> =
        core("removeLabel") { it.removeLabel(pr.repo, pr.n, label) }

    override suspend fun addComment(pr: PrRef, body: String): Outcome<Unit> =
        core("addComment") { it.addComment(pr.repo, pr.n, body) }

    override fun renderMarkdown(source: String, repo: String): Outcome<List<MdBlock>> = try {
        ffiCall("renderMarkdown") { ffiRenderMarkdown(source, repo).toModel() }
    } catch (e: LinkageError) {
        Outcome.Err(BackendError.Internal("the core library could not be loaded: ${e.message ?: e.javaClass.simpleName}"))
    }

    override suspend fun replyToThread(pr: PrRef, threadId: String, body: String): Outcome<Unit> =
        core("replyToThread") { it.replyToThread(pr.repo, pr.n, threadId, body) }

    override suspend fun merge(
        pr: PrRef,
        method: MergeMethod,
        commitTitle: String?,
        commitMessage: String?,
        expectedHeadSha: String,
    ): Outcome<Unit> = core("merge") {
        it.merge(pr.repo, pr.n, method.toFfi(), commitTitle, commitMessage, expectedHeadSha)
    }

    override suspend fun closePullRequest(pr: PrRef): Outcome<Unit> =
        core("closePullRequest") { it.closePullRequest(pr.repo, pr.n) }

    override suspend fun reopenPullRequest(pr: PrRef): Outcome<Unit> =
        core("reopenPullRequest") { it.reopenPullRequest(pr.repo, pr.n) }

    override suspend fun setDraft(pr: PrRef, draft: Boolean): Outcome<Unit> =
        core("setDraft") { it.setDraft(pr.repo, pr.n, draft) }

    override suspend fun updateBranch(pr: PrRef, method: BranchUpdateMethod, expectedHeadOid: String): Outcome<Unit> =
        core("updateBranch") { it.updateBranch(pr.repo, pr.n, method.toFfi(), expectedHeadOid) }

    // --- files and diff ------------------------------------------------------

    override suspend fun filesOverview(pr: PrRef): Outcome<FilesOverview> =
        core("filesOverview") { it.filesOverview(pr.repo, pr.n).toModel() }

    override suspend fun fileDiff(pr: PrRef, fileIndex: Int): Outcome<FileDiff> =
        if (fileIndex < 0) {
            invalid("No file #$fileIndex in this pull request")
        } else {
            core("fileDiff") { it.fileDiff(pr.repo, pr.n, fileIndex.toUInt()).toModel() }
        }

    // --- pending review ------------------------------------------------------

    override suspend fun pendingReview(pr: PrRef): Outcome<PendingReview> =
        core("pendingReview") { it.pendingReview(pr.repo, pr.n).toModel() }

    override suspend fun addDraft(
        pr: PrRef,
        anchor: CommentAnchor,
        rangeStart: CommentAnchor?,
        body: String,
    ): Outcome<PendingReview> = core("addDraft") {
        it.addDraft(pr.repo, pr.n, anchor.toFfi(), rangeStart?.toFfi(), body).toModel()
    }

    override suspend fun editDraft(pr: PrRef, draftId: Long, body: String): Outcome<PendingReview> =
        core("editDraft") { it.editDraft(pr.repo, pr.n, draftId.toULong(), body).toModel() }

    override suspend fun removeDraft(pr: PrRef, draftId: Long): Outcome<PendingReview> =
        core("removeDraft") { it.removeDraft(pr.repo, pr.n, draftId.toULong()).toModel() }

    override suspend fun discardDrafts(pr: PrRef): Outcome<PendingReview> =
        core("discardDrafts") { it.discardDrafts(pr.repo, pr.n).toModel() }

    override suspend fun submitReview(pr: PrRef, event: ReviewEvent, body: String, includeDrafts: Boolean): Outcome<Unit> =
        core("submitReview") { it.submitReview(pr.repo, pr.n, event.toFfi(), body, includeDrafts) }

    // --- desktop -------------------------------------------------------------

    override suspend fun parsePairingLink(uri: String): Outcome<PairingPreview> =
        core("parsePairingLink") { it.parsePairingLink(uri).toModel() }

    override suspend fun pairWithLink(uri: String, deviceName: String): Outcome<PairingResult> =
        core("pairWithLink") { it.pairWithLink(uri, deviceName).toModel() }

    override suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> =
        if (port !in 1..65535) {
            invalid("Ports run from 1 to 65535")
        } else {
            core("probeDesktop") { it.probeDesktop(host, port.toUShort()).toModel() }
        }

    override suspend fun pairManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<PairingResult> =
        if (port !in 1..65535) {
            invalid("Ports run from 1 to 65535")
        } else {
            core("pairManual") { it.pairManual(host, port.toUShort(), fingerprint, code, deviceName).toModel() }
        }

    override suspend fun setRemote(endpoint: String, deviceToken: String): Outcome<RemoteStatus> =
        core("setRemote") { it.setRemote(endpoint, deviceToken).toModel() }

    override suspend fun clearRemote(): Outcome<Unit> = core("clearRemote") { it.clearRemote() }

    override suspend fun remoteStatus(): Outcome<RemoteStatus> = core("remoteStatus") { it.remoteStatus().toModel() }

    override suspend fun machineInfo(): Outcome<MachineInfo> = core("machineInfo") { it.machineInfo().toModel() }

    override suspend fun localStatus(pr: PrRef): Outcome<LocalStatus> =
        core("localStatus") { it.localStatus(pr.repo, pr.n).toModel() }

    override suspend fun runLocalJob(pr: PrRef, op: LocalOp, autostash: Boolean): Outcome<JobResult> =
        core("runLocalJob") { it.runLocalJob(pr.repo, pr.n, op.toFfi(), autostash).toModel() }

    override suspend fun abortLocal(pr: PrRef): Outcome<Unit> = core("abortLocal") { it.abortLocal(pr.repo, pr.n) }

    override suspend fun startSyncAll(op: SyncAllOp, autostash: Boolean): Outcome<SyncRun> =
        core("startSyncAll") { it.startSyncAll(op.toFfi(), autostash).toModel() }

    override suspend fun syncAllStatus(): Outcome<SyncRun?> = core("syncAllStatus") { it.syncAllStatus()?.toModel() }

    override suspend fun handoffs(): Outcome<List<HandoffSession>> =
        core("handoffs") { it.handoffs().map { session -> session.toModel() } }

    override suspend fun refreshGitHubTokenFromDesktop(): Outcome<DesktopGitHubToken> =
        core("refreshGithubTokenFromDesktop") { it.refreshGithubTokenFromDesktop().toModel() }

    override suspend fun unpair(): Outcome<Unit> = core("unpair") { it.unpair() }

    // --- desktop settings ----------------------------------------------------

    override suspend fun desktopConfig(): Outcome<DesktopConfigPreview> =
        core("desktopConfig") { it.desktopConfig().toModel() }

    override suspend fun copyDesktopConfig(): Outcome<Settings> =
        core("copyDesktopConfig") { it.copyDesktopConfig().toModel() }

    // --- notifications -------------------------------------------------------

    override suspend fun checkNotifications(): Outcome<List<NotificationEvent>> =
        core("checkNotifications") { it.checkNotifications().map { event -> event.toModel() } }

    override suspend fun markNotificationsSeen(): Outcome<Unit> = core("markNotificationsSeen") { it.markNotificationsSeen() }
}
