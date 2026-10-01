package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.IssueCloseReason
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.IssueStatus
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.TimelineEntry
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Clock
import java.time.Instant

/**
 * The fake's issues: the Issues tab's rows, the issue screen and every
 * action on it. Owned by [FakeRostrumBackend], which serialises access,
 * checks sign-in and injects failures; [onChanged] republishes the feed.
 */
internal class FakeIssues(
    private val clock: Clock,
    started: Instant,
    private val labelsOf: (String) -> Map<String, LabelView>,
    /** The highest pull request number in a repository, so a new issue takes the next one. */
    private val highestPullNumber: (String) -> Int,
    private val onChanged: () -> Unit,
) {
    val issues: MutableMap<IssueRef, FakeIssue> =
        SampleIssues.issues(started).associateByTo(LinkedHashMap()) { it.ref }
    private val timelines = mutableMapOf<IssueRef, MutableList<TimelineEntry>>()
    private var nextId = 1L

    /** Open issues per repository, among [repos]. */
    fun open(repos: List<String>): Map<String, List<FakeIssue>> =
        issues.values.filter { it.isOpen && it.repo in repos }.groupBy { it.repo }

    private fun notFound(issue: IssueRef) = Outcome.Err(BackendError.GitHubApi(404, "Could not resolve to an issue: $issue"))

    private fun timeline(issue: FakeIssue): MutableList<TimelineEntry> = timelines.getOrPut(issue.ref) {
        buildList {
            add(TimelineEntry("issue-body-${issue.number}", UserRef(issue.author), issue.createdAt,
                TimelineKind.Description(FakeMarkdown.parse(issue.body), issue.body)))
            repeat(issue.comments) { index ->
                val text = if (index % 2 == 0) "Seeing this too." else "I can take a look at this."
                add(TimelineEntry("issue-comment-${issue.number}-$index", UserRef(if (index % 2 == 0) "mkowal" else "ada-lin"),
                    issue.updatedAt, TimelineKind.Comment(FakeMarkdown.parse(text), text)))
            }
        }.toMutableList()
    }

    private fun detailOf(issue: FakeIssue, viewer: String?) =
        IssueDetail(issue.summary(viewer, labelsOf(issue.repo)), timeline(issue).toList())

    fun detail(ref: IssueRef, viewer: String?): Outcome<IssueDetail> =
        issues[ref]?.let { Outcome.Ok(detailOf(it, viewer)) } ?: notFound(ref)

    /** Only an issue whose screen was fetched before is cached. */
    fun cached(ref: IssueRef, viewer: String?): IssueDetail? =
        issues[ref]?.takeIf { ref in timelines }?.let { detailOf(it, viewer) }

    private fun event(issue: FakeIssue, event: TimelineEvent, text: String) {
        timeline(issue) += TimelineEntry("issue-event-${nextId++}", UserRef(SamplePulls.VIEWER), clock.instant(),
            TimelineKind.Event(event, text))
    }

    private inline fun change(ref: IssueRef, block: (FakeIssue) -> Outcome<FakeIssue>): Outcome<Unit> {
        val issue = issues[ref] ?: return notFound(ref)
        return when (val next = block(issue)) {
            is Outcome.Err -> next
            is Outcome.Ok -> {
                issues[ref] = next.value.copy(updatedAt = clock.instant())
                onChanged()
                Outcome.Ok(Unit)
            }
        }
    }

    fun comment(ref: IssueRef, body: String): Outcome<Unit> = change(ref) { issue ->
        if (body.isBlank()) return@change Outcome.Err(BackendError.InvalidInput("A comment can't be empty"))
        timeline(issue) += TimelineEntry("issue-comment-${nextId++}", UserRef(SamplePulls.VIEWER), clock.instant(),
            TimelineKind.Comment(FakeMarkdown.parse(body), body))
        Outcome.Ok(issue.copy(comments = issue.comments + 1))
    }

    fun close(ref: IssueRef, reason: CloseIssueAs): Outcome<Unit> = change(ref) { issue ->
        if (!issue.isOpen) return@change Outcome.Err(BackendError.GitHubApi(422, "Issue is already closed"))
        val (closeReason, words) = when (reason) {
            CloseIssueAs.Completed -> IssueCloseReason.Completed to "completed"
            CloseIssueAs.NotPlanned -> IssueCloseReason.NotPlanned to "not planned"
        }
        event(issue, TimelineEvent.ClosedAs(closeReason), "closed this as $words")
        Outcome.Ok(issue.copy(status = IssueStatus.Closed(closeReason)))
    }

    fun reopen(ref: IssueRef): Outcome<Unit> = change(ref) { issue ->
        if (issue.isOpen) return@change Outcome.Err(BackendError.GitHubApi(422, "Issue is not closed"))
        event(issue, TimelineEvent.Reopened, "reopened this")
        Outcome.Ok(issue.copy(status = IssueStatus.Open))
    }

    fun addLabel(ref: IssueRef, label: String): Outcome<Unit> = change(ref) { issue ->
        if (label in issue.labels) return@change Outcome.Ok(issue)
        event(issue, TimelineEvent.Labeled(label), "added the $label label")
        Outcome.Ok(issue.copy(labels = issue.labels + label))
    }

    fun removeLabel(ref: IssueRef, label: String): Outcome<Unit> = change(ref) { issue ->
        if (label !in issue.labels) return@change Outcome.Ok(issue)
        event(issue, TimelineEvent.Unlabeled(label), "removed the $label label")
        Outcome.Ok(issue.copy(labels = issue.labels - label))
    }

    fun addAssignee(ref: IssueRef, login: String): Outcome<Unit> = change(ref) { issue ->
        if (issue.assignees.any { it.equals(login, ignoreCase = true) }) return@change Outcome.Ok(issue)
        event(issue, TimelineEvent.Assigned(login), "assigned $login")
        Outcome.Ok(issue.copy(assignees = issue.assignees + login))
    }

    fun removeAssignee(ref: IssueRef, login: String): Outcome<Unit> = change(ref) { issue ->
        if (issue.assignees.none { it.equals(login, ignoreCase = true) }) return@change Outcome.Ok(issue)
        event(issue, TimelineEvent.Unassigned(login), "unassigned $login")
        Outcome.Ok(issue.copy(assignees = issue.assignees.filterNot { it.equals(login, ignoreCase = true) }))
    }

    fun create(repo: String, title: String, body: String, labels: List<String>, assignees: List<String>): Outcome<Int> {
        val cleanTitle = title.trim()
        if (cleanTitle.isEmpty()) return Outcome.Err(BackendError.InvalidInput("Give the issue a title"))
        val number = maxOf(highestPullNumber(repo), issues.keys.filter { it.repo == repo }.maxOfOrNull { it.number } ?: 0) + 1
        val now = clock.instant()
        issues[IssueRef(repo, number)] = FakeIssue(
            repo = repo,
            number = number,
            title = cleanTitle,
            body = body.trim(),
            author = SamplePulls.VIEWER,
            createdAt = now,
            updatedAt = now,
            labels = labels.distinct(),
            assignees = assignees.distinctBy { it.lowercase() },
        )
        onChanged()
        return Outcome.Ok(number)
    }
}
