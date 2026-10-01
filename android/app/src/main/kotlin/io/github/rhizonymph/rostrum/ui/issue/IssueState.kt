package io.github.rhizonymph.rostrum.ui.issue

import io.github.rhizonymph.rostrum.data.model.CloseIssueAs
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueStatus
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.PickerState
import java.time.Instant

/** The state-changing actions the issue screen offers, from its menu. */
sealed interface IssueStateAction {
    data class Close(val reason: CloseIssueAs) : IssueStateAction

    data object Reopen : IssueStateAction
}

/** The menu's entries for an issue: close (two ways) while open, reopen once closed. */
fun stateActions(issue: IssueSummary): List<IssueStateAction> = when (issue.status) {
    IssueStatus.Open -> listOf(IssueStateAction.Close(CloseIssueAs.Completed), IssueStateAction.Close(CloseIssueAs.NotPlanned))
    is IssueStatus.Closed -> listOf(IssueStateAction.Reopen)
}

fun actionLabel(action: IssueStateAction): String = when (action) {
    IssueStateAction.Close(CloseIssueAs.Completed) -> "Close as completed"
    IssueStateAction.Close(CloseIssueAs.NotPlanned) -> "Close as not planned"
    is IssueStateAction.Close -> "Close"
    IssueStateAction.Reopen -> "Reopen"
}

/** What the snackbar says once the action went through. */
fun actionDone(action: IssueStateAction, number: Int): String = when (action) {
    is IssueStateAction.Close -> when (action.reason) {
        CloseIssueAs.Completed -> "Closed #$number as completed"
        CloseIssueAs.NotPlanned -> "Closed #$number as not planned"
    }
    IssueStateAction.Reopen -> "Reopened #$number"
}

/** `ada-lin opened 3h ago`, with the milestone when there is one. */
fun issueByline(issue: IssueSummary, age: String): String = buildString {
    append(issue.author?.login ?: "ghost")
    append(" opened $age")
    issue.milestone?.let { append(" · $it") }
}

/** Everything the issue screen renders. */
data class IssueUiState(
    val detail: UiState<IssueDetail> = UiState.Loading,
    /** A fresh fetch is in flight while an older copy is shown. */
    val refreshing: Boolean = false,
    val comment: String = "",
    val sending: Boolean = false,
    /** Close or reopen, while it runs (or after it failed). */
    val stateAction: ActionState = ActionState.Idle,
    /** The open labels or assignees picker, if any. */
    val picker: PickerState? = null,
    val now: Instant,
) {
    val issue: IssueSummary? get() = (detail as? UiState.Loaded)?.data?.issue

    /** What the open picker shows checked. */
    val pickerSelection: Set<String>
        get() = when (picker?.kind) {
            PickerKind.Labels -> issue?.labels.orEmpty().mapTo(mutableSetOf()) { it.name }
            PickerKind.Assignees -> issue?.assignees.orEmpty().mapTo(mutableSetOf()) { it.login }
            null -> emptySet()
        }
}
