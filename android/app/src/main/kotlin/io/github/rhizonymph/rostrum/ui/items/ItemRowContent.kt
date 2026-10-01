package io.github.rhizonymph.rostrum.ui.items

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import java.time.Instant

/** One [ItemRow], drawn: a pull request (maybe a stack member), a stack header, or an issue. */
@Composable
fun ItemRowContent(
    row: ItemRow,
    now: Instant,
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenIssue: (IssueRef) -> Unit,
    modifier: Modifier = Modifier,
) {
    when (row) {
        is ItemRow.Pull -> PrRow(row.pr, now, onClick = { onOpenPullRequest(row.pr.ref) }, modifier = modifier, stack = row.stack)
        is ItemRow.StackHeader -> StackHeaderRow(row.stack, modifier)
        is ItemRow.Issue -> IssueRow(row.issue, now, onClick = { onOpenIssue(row.issue.ref) }, modifier = modifier)
    }
}
