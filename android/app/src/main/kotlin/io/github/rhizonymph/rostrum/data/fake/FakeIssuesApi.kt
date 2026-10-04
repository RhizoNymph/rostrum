package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.IssuesApi
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Instant

/** The fake's [IssuesApi]: each call through the owner's plumbing, onto [FakeIssues]. */
internal class FakeIssuesApi(private val host: FakeHost, private val issues: FakeIssues) : IssuesApi {
    override suspend fun issueDetail(issue: IssueRef): Outcome<IssueDetail> =
        host.call(FakeCall.IssueDetail) { host.signedIn { issues.detail(issue, host.viewerLogin) } }

    override suspend fun cachedIssueDetail(issue: IssueRef): Outcome<IssueDetail?> =
        host.call(FakeCall.CachedIssueDetail) { Outcome.Ok(issues.cached(issue, host.viewerLogin)) }

    override suspend fun loadEarlierIssue(issue: IssueRef): Outcome<IssueDetail> =
        host.call(FakeCall.LoadEarlierIssue) { host.signedIn { issues.loadEarlier(issue, host.viewerLogin) } }

    override suspend fun editIssue(
        issue: IssueRef,
        title: String,
        body: String,
        baseUpdatedAt: Instant,
        overwrite: Boolean,
    ): Outcome<IssueDetail> =
        host.call(FakeCall.EditIssue) { host.signedIn { issues.edit(issue, title, body, baseUpdatedAt, overwrite, host.viewerLogin) } }

    override suspend fun commentOnIssue(issue: IssueRef, body: String): Outcome<Unit> =
        host.call(FakeCall.CommentOnIssue) { host.signedIn { issues.comment(issue, body) } }

    override suspend fun closeIssue(issue: IssueRef, reason: CloseIssueAs): Outcome<Unit> =
        host.call(FakeCall.CloseIssue) { host.signedIn { issues.close(issue, reason) } }

    override suspend fun reopenIssue(issue: IssueRef): Outcome<Unit> =
        host.call(FakeCall.ReopenIssue) { host.signedIn { issues.reopen(issue) } }

    override suspend fun addIssueLabel(issue: IssueRef, label: String): Outcome<Unit> =
        host.call(FakeCall.AddIssueLabel) { host.signedIn { issues.addLabel(issue, label) } }

    override suspend fun removeIssueLabel(issue: IssueRef, label: String): Outcome<Unit> =
        host.call(FakeCall.RemoveIssueLabel) { host.signedIn { issues.removeLabel(issue, label) } }

    override suspend fun assignableUsers(repo: String): Outcome<List<UserRef>> =
        host.call(FakeCall.AssignableUsers) { host.signedIn { Outcome.Ok(SampleIssues.assignable(repo)) } }

    override suspend fun addIssueAssignee(issue: IssueRef, login: String): Outcome<Unit> =
        host.call(FakeCall.AddIssueAssignee) { host.signedIn { issues.addAssignee(issue, login) } }

    override suspend fun removeIssueAssignee(issue: IssueRef, login: String): Outcome<Unit> =
        host.call(FakeCall.RemoveIssueAssignee) { host.signedIn { issues.removeAssignee(issue, login) } }

    override suspend fun createIssue(
        repo: String,
        title: String,
        body: String,
        labels: List<String>,
        assignees: List<String>,
    ): Outcome<Int> = host.call(FakeCall.CreateIssue) { host.signedIn { issues.create(repo, title, body, labels, assignees) } }
}
