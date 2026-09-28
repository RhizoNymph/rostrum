package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.BranchUpdateMethod
import io.github.rhizonymph.rostrum.data.model.CommentAnchor
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
import kotlinx.coroutines.flow.Flow

/**
 * Everything the UI asks of the Rust core, shaped after `RostrumCore` in
 * `rostrum-ffi` method for method. ViewModels depend on this interface only.
 *
 * Every call is main-safe (the core's exports are async and never block the
 * caller) and reports failure as [Outcome.Err] rather than throwing.
 * Secrets are never persisted here: the app keeps them in its secret store and
 * hands them in with [setGitHubToken] and [setRemote] at start-up.
 */
interface RostrumBackend {
    // --- session -------------------------------------------------------------

    /**
     * Hand in the GitHub token, or `null` to sign out. Held in memory only.
     * The core talks to github.com only; GitHub Enterprise is not supported.
     */
    suspend fun setGitHubToken(token: String?): Outcome<GitHubStatus>

    /** The token's current status, without a network request. */
    suspend fun githubStatus(): GitHubStatus

    /** Who the token belongs to; asks GitHub when not yet known. */
    suspend fun viewer(): Outcome<UserRef>

    /** Problems found while loading the settings file. */
    suspend fun warnings(): List<String>

    // --- settings ------------------------------------------------------------

    suspend fun settings(): Outcome<Settings>

    /**
     * Add a repository from `owner/name` or a GitHub URL, returning its
     * normalised `owner/name`. Fails with [BackendError.InvalidRepo] or
     * [BackendError.DuplicateRepo].
     */
    suspend fun addRepo(input: String): Outcome<String>

    /** Stop watching a repository. Returns whether it was watched. */
    suspend fun removeRepo(repo: String): Outcome<Boolean>

    suspend fun setRefreshInterval(seconds: Long): Outcome<Settings>

    suspend fun setPrsPerRepo(count: Int): Outcome<Settings>

    suspend fun setNotifications(newPullRequests: Boolean, reviewRequests: Boolean): Outcome<Settings>

    suspend fun setAutostash(autostash: Boolean): Outcome<Settings>

    // --- feed ----------------------------------------------------------------

    /** The feed from memory or the cache. No network; paints the first frame. */
    suspend fun cachedFeed(): Outcome<FeedSnapshot>

    /** Fetch every watched repository. Fails only for sign-in problems. */
    suspend fun refreshFeed(): Outcome<FeedSnapshot>

    /** Fetch one repository, e.g. just after adding it. */
    suspend fun refreshRepo(repo: String): Outcome<FeedSnapshot>

    /** Set the search box (title, number, author, labels). Not persisted. */
    suspend fun setQuery(query: String): Outcome<FeedSnapshot>

    /** Replace the persisted filter preferences. */
    suspend fun setFilter(preferences: FeedPreferences): Outcome<FeedSnapshot>

    /** Add or remove one author from the filter (case-insensitive). */
    suspend fun toggleAuthor(login: String): Outcome<FeedSnapshot>

    /** Reset the query and every preference to their defaults. */
    suspend fun clearFilter(): Outcome<FeedSnapshot>

    /** Collapse or expand a repository's container. Not persisted. */
    suspend fun toggleCollapsed(repo: String): Outcome<FeedSnapshot>

    /** People the author filter can be pointed at; `limit` caps the unselected tail. */
    suspend fun authorRoster(limit: Int?): Outcome<AuthorRoster>

    /** Every feed change, including background ones (the core's `FeedObserver`). */
    val feedUpdates: Flow<FeedSnapshot>

    // --- one pull request ----------------------------------------------------

    /** Fetch the conversation, threads and checks from GitHub. */
    suspend fun pullDetail(pr: PrRef): Outcome<PullDetail>

    /** The last fetched detail from the cache, or `null`. No network. */
    suspend fun cachedPullDetail(pr: PrRef): Outcome<PullDetail?>

    /** Just the header, from the feed's data. No network. */
    suspend fun pullHeader(pr: PrRef): Outcome<PullHeader>

    /** Every label defined on the repository, for the label picker. */
    suspend fun repositoryLabels(repo: String): Outcome<List<LabelView>>

    suspend fun addLabel(pr: PrRef, label: String): Outcome<Unit>

    suspend fun removeLabel(pr: PrRef, label: String): Outcome<Unit>

    /** Post a top-level conversation comment. */
    suspend fun addComment(pr: PrRef, body: String): Outcome<Unit>

    /**
     * Render markdown exactly as the timeline will show it, for the
     * composer's Preview. [repo] (`owner/name`) resolves `#123` shorthand.
     * Synchronous and cheap for comment-sized text.
     */
    fun renderMarkdown(source: String, repo: String): Outcome<List<MdBlock>>

    /** Reply into an inline thread. */
    suspend fun replyToThread(pr: PrRef, threadId: String, body: String): Outcome<Unit>

    /**
     * Merge; confirm in the UI first. GitHub refuses if the head moved from
     * [expectedHeadSha]. Blank title or message means GitHub's default.
     */
    suspend fun merge(
        pr: PrRef,
        method: MergeMethod,
        commitTitle: String?,
        commitMessage: String?,
        expectedHeadSha: String,
    ): Outcome<Unit>

    /** Close without merging; confirm in the UI first. */
    suspend fun closePullRequest(pr: PrRef): Outcome<Unit>

    /** Reopen a closed pull request; confirm in the UI first. */
    suspend fun reopenPullRequest(pr: PrRef): Outcome<Unit>

    /** Into (`true`) or out of draft. Reversible, so no confirmation. */
    suspend fun setDraft(pr: PrRef, draft: Boolean): Outcome<Unit>

    /** Bring the branch up to date with its base on GitHub. */
    suspend fun updateBranch(pr: PrRef, method: BranchUpdateMethod, expectedHeadOid: String): Outcome<Unit>

    // --- files and diff ------------------------------------------------------

    suspend fun filesOverview(pr: PrRef): Outcome<FilesOverview>

    /** One file's diff as rows; [fileIndex] is `ChangedFile.index`. */
    suspend fun fileDiff(pr: PrRef, fileIndex: Int): Outcome<FileDiff>

    // --- pending review ------------------------------------------------------

    suspend fun pendingReview(pr: PrRef): Outcome<PendingReview>

    /** Add an inline draft; [rangeStart] is the first line's anchor for a range. */
    suspend fun addDraft(
        pr: PrRef,
        anchor: CommentAnchor,
        rangeStart: CommentAnchor?,
        body: String,
    ): Outcome<PendingReview>

    suspend fun editDraft(pr: PrRef, draftId: Long, body: String): Outcome<PendingReview>

    suspend fun removeDraft(pr: PrRef, draftId: Long): Outcome<PendingReview>

    suspend fun discardDrafts(pr: PrRef): Outcome<PendingReview>

    /** Submit a review, optionally with the pending drafts as inline comments. */
    suspend fun submitReview(
        pr: PrRef,
        event: ReviewEvent,
        body: String,
        includeDrafts: Boolean,
    ): Outcome<Unit>

    // --- desktop -------------------------------------------------------------

    /** Read a `rostrum://pair?…` link without contacting anything. */
    suspend fun parsePairingLink(uri: String): Outcome<PairingPreview>

    /** Pair using a link. Persist the result's secrets. */
    suspend fun pairWithLink(uri: String, deviceName: String): Outcome<PairingResult>

    /** Ask a desktop typed in by address who it is and which certificate it presents. */
    suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe>

    /** Pair by address and typed code, pinned to the probed [fingerprint]. */
    suspend fun pairManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<PairingResult>

    /** Use a previously paired desktop for this session. */
    suspend fun setRemote(endpoint: String, deviceToken: String): Outcome<RemoteStatus>

    /** Forget the desktop for this session. */
    suspend fun clearRemote(): Outcome<Unit>

    suspend fun remoteStatus(): Outcome<RemoteStatus>

    suspend fun machineInfo(): Outcome<MachineInfo>

    /** The desktop clone's view of a pull request's branch (fetches first). */
    suspend fun localStatus(pr: PrRef): Outcome<LocalStatus>

    /** Run one local operation on the pull request's worktree and wait for it. */
    suspend fun runLocalJob(pr: PrRef, op: LocalOp, autostash: Boolean): Outcome<JobResult>

    /** Abort the rebase or merge stopped in the pull request's worktree. */
    suspend fun abortLocal(pr: PrRef): Outcome<Unit>

    /** Start "sync all"; returns at once. Poll [syncAllStatus]. */
    suspend fun startSyncAll(op: SyncAllOp, autostash: Boolean): Outcome<SyncRun>

    /** The latest "sync all", running or finished, or `null`. */
    suspend fun syncAllStatus(): Outcome<SyncRun?>

    /** tmux sessions on the desktop holding stopped operations. */
    suspend fun handoffs(): Outcome<List<HandoffSession>>

    /** Ask the desktop for its current GitHub token. Persist the result. */
    suspend fun refreshGitHubTokenFromDesktop(): Outcome<DesktopGitHubToken>

    /** Unpair on the desktop, then forget it here. */
    suspend fun unpair(): Outcome<Unit>

    // --- notifications -------------------------------------------------------

    /** Refresh and report what is new since the last check. */
    suspend fun checkNotifications(): Outcome<List<NotificationEvent>>

    /** Fold the current feed into the seen set without reporting it. */
    suspend fun markNotificationsSeen(): Outcome<Unit>

    companion object {
        const val GITHUB_COM = "github.com"
    }
}
