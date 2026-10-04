package io.github.rhizonymph.rostrum.ui.items

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PrSummary
import java.time.Instant

/** What a row can ask for: open a pull request or issue, a stack action, or (in Arrange mode) picking. */
class RowCallbacks(
    val openPullRequest: (PrRef) -> Unit,
    val openIssue: (IssueRef) -> Unit,
    /** A stack header's menu entry; `null` hides the menu. */
    val stackAction: ((ItemRow.StackHeader, StackMenuEntry) -> Unit)? = null,
    /** In Arrange mode, the picked pull requests (in order); `null` when not picking. */
    val picked: List<Int>? = null,
    val togglePicked: (PrSummary) -> Unit = {},
)

/** One [ItemRow], drawn: a pull request (maybe a stack member), a stack header, or an issue. */
@Composable
fun ItemRowContent(row: ItemRow, now: Instant, callbacks: RowCallbacks, modifier: Modifier = Modifier) {
    when (row) {
        is ItemRow.Pull -> {
            val picked = callbacks.picked
            if (picked != null) {
                PrRow(row.pr, now, onClick = { callbacks.togglePicked(row.pr) }, modifier = modifier, stack = row.stack,
                    pick = picked.indexOf(row.pr.number).takeIf { it >= 0 }?.plus(1) ?: 0)
            } else {
                PrRow(row.pr, now, onClick = { callbacks.openPullRequest(row.pr.ref) }, modifier = modifier, stack = row.stack)
            }
        }
        is ItemRow.StackHeader -> StackHeaderRow(
            row.stack,
            modifier,
            onAction = callbacks.stackAction?.let { act -> { entry -> act(row, entry) } },
        )
        is ItemRow.Issue -> IssueRow(row.issue, now, onClick = { callbacks.openIssue(row.issue.ref) }, modifier = modifier)
    }
}
