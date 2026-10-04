package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Instant

/** The issue part of [RostrumBackend]: the issue screen, its actions, and opening an issue. */
interface IssuesApi {

    /** The issue with its timeline, from GitHub. */
    suspend fun issueDetail(issue: IssueRef): Outcome<IssueDetail>

    /** The last fetched issue screen, if any; never contacts GitHub. */
    suspend fun cachedIssueDetail(issue: IssueRef): Outcome<IssueDetail?>

    /** The issue with its next earlier page of timeline merged in; a reload keeps it. */
    suspend fun loadEarlierIssue(issue: IssueRef): Outcome<IssueDetail>

    /**
     * Change the title and description. [baseUpdatedAt] is the opened issue's
     * `updatedAt`: a title or description changed on GitHub since is
     * [BackendError.EditConflict], unless [overwrite]. Unchanged sends
     * nothing; a blank title is refused. Answers the issue as saved.
     */
    suspend fun editIssue(issue: IssueRef, title: String, body: String, baseUpdatedAt: Instant, overwrite: Boolean): Outcome<IssueDetail>

    suspend fun commentOnIssue(issue: IssueRef, body: String): Outcome<Unit>

    suspend fun closeIssue(issue: IssueRef, reason: CloseIssueAs): Outcome<Unit>

    suspend fun reopenIssue(issue: IssueRef): Outcome<Unit>

    suspend fun addIssueLabel(issue: IssueRef, label: String): Outcome<Unit>

    suspend fun removeIssueLabel(issue: IssueRef, label: String): Outcome<Unit>

    /** Who issues in [repo] can be assigned to. */
    suspend fun assignableUsers(repo: String): Outcome<List<UserRef>>

    suspend fun addIssueAssignee(issue: IssueRef, login: String): Outcome<Unit>

    suspend fun removeIssueAssignee(issue: IssueRef, login: String): Outcome<Unit>

    /** Open an issue; answers its number. A blank title is refused. */
    suspend fun createIssue(
        repo: String,
        title: String,
        body: String,
        labels: List<String>,
        assignees: List<String>,
    ): Outcome<Int>
}
