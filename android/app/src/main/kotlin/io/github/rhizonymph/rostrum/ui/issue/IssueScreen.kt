package io.github.rhizonymph.rostrum.ui.issue

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.IssueDetail
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.IssueSummary
import io.github.rhizonymph.rostrum.data.model.TimelineEntry
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.CommentBar
import io.github.rhizonymph.rostrum.ui.components.CommentCard
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.EventRow
import io.github.rhizonymph.rostrum.ui.components.LabelChip
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.TitleStack
import io.github.rhizonymph.rostrum.ui.components.eventIcon
import io.github.rhizonymph.rostrum.ui.format.relativeAge
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** The issue screen, stateless: header card, labels and assignees, timeline, composer. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IssueScreen(
    ref: IssueRef,
    state: IssueUiState,
    actions: IssueActions,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxSize().background(colors.bg).imePadding()) {
        BackTopBar(
            onBack = onBack,
            actions = { state.issue?.let { IssueMenu(it, busy = state.stateAction.running, onAction = actions::run) } },
        ) {
            TitleStack(ref.repo) {
                Text("Issue #${ref.number}", style = RostrumText.sheetTitle, color = colors.text, maxLines = 1)
            }
        }
        PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = actions::refresh, modifier = Modifier.weight(1f).fillMaxWidth()) {
            when (val detail = state.detail) {
                UiState.Loading -> LoadingView(label = "Loading the issue…")
                is UiState.Error -> ErrorView(detail.error, Modifier.padding(12.dp), title = "Couldn't load this issue", onRetry = actions::retry)
                is UiState.Loaded -> IssueBody(detail.data, state.now, actions)
            }
        }
        if (state.issue != null) {
            CommentBar(
                text = state.comment,
                onTextChange = actions::onCommentChange,
                onSend = actions::sendComment,
                sending = state.sending,
            )
        }
    }
}

@Composable
private fun IssueBody(detail: IssueDetail, now: Instant, actions: IssueActions) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "header") { IssueHeader(detail.issue, now, actions) }
        items(detail.timeline, key = { it.id }) { entry -> TimelineEntryView(entry, now) }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun IssueHeader(issue: IssueSummary, now: Instant, actions: IssueActions) {
    val colors = RostrumTheme.colors
    RostrumCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(
                issue.title,
                style = RostrumText.sheetTitle.copy(fontWeight = FontWeight.SemiBold),
                color = colors.text,
                modifier = Modifier.semantics { heading() },
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                StatusChip(issue.statusChip)
                Text(issueByline(issue, relativeAge(issue.createdAt, now)), style = RostrumText.meta, color = colors.textMuted)
            }
            HeaderList("Labels", onEdit = { actions.openPicker(PickerKind.Labels) }, empty = issue.labels.isEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    issue.labels.forEach { LabelChip(it) }
                }
            }
            HeaderList("Assignees", onEdit = { actions.openPicker(PickerKind.Assignees) }, empty = issue.assignees.isEmpty()) {
                Text(issue.assignees.joinToString(", ") { it.login }, style = RostrumText.meta, color = colors.textSecondary)
            }
        }
    }
}

/** "Labels  [chips]  Edit", or "None yet" when the list is empty. */
@Composable
private fun HeaderList(title: String, onEdit: () -> Unit, empty: Boolean, content: @Composable () -> Unit) {
    val colors = RostrumTheme.colors
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = RostrumText.caption, color = colors.textMuted, modifier = Modifier.padding(end = 4.dp))
        Box(Modifier.weight(1f)) {
            if (empty) Text("None yet", style = RostrumText.meta, color = colors.textSubtle) else content()
        }
        TextPillButton("Edit", onEdit)
    }
}

@Composable
private fun TimelineEntryView(entry: TimelineEntry, now: Instant) {
    val author = entry.author?.login ?: "ghost"
    when (val kind = entry.kind) {
        is TimelineKind.Description -> CommentCard(author, entry.createdAt, now, kind.body)
        is TimelineKind.Comment -> CommentCard(author, entry.createdAt, now, kind.body)
        is TimelineKind.Review -> CommentCard(author, entry.createdAt, now, kind.body, chip = kind.chip)
        is TimelineKind.Event -> EventRow(eventIcon(kind.event), author, kind.text, relativeAge(entry.createdAt, now))
    }
}

/** Close as completed / not planned while open; Reopen once closed. */
@Composable
private fun IssueMenu(issue: IssueSummary, busy: Boolean, onAction: (IssueStateAction) -> Unit) {
    val colors = RostrumTheme.colors
    var open by remember { mutableStateOf(false) }
    Box {
        RostrumIconButton(RostrumIcons.MoreVert, "Issue actions", onClick = { open = true }, iconSize = 20.dp, enabled = !busy)
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = colors.raised) {
            stateActions(issue).forEach { action ->
                DropdownMenuItem(
                    text = { Text(actionLabel(action), style = RostrumText.label, color = colors.text) },
                    onClick = {
                        open = false
                        onAction(action)
                    },
                )
            }
        }
    }
}
