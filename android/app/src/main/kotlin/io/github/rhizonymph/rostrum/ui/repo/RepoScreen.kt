package io.github.rhizonymph.rostrum.ui.repo

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.BranchRow
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.BranchTree
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.RepoOverview
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.NewIssueFab
import io.github.rhizonymph.rostrum.ui.components.SegmentPosition
import io.github.rhizonymph.rostrum.ui.components.SegmentedToggle
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.TitleStack
import io.github.rhizonymph.rostrum.ui.components.cardSegment
import io.github.rhizonymph.rostrum.ui.items.ItemRow
import io.github.rhizonymph.rostrum.ui.items.ItemRowContent
import io.github.rhizonymph.rostrum.ui.items.rowsOf
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** `Pull requests 3`: a tab's title with its count once known. */
fun repoTabLabel(tab: RepoTab, overview: RepoOverview?): String {
    val count = when (tab) {
        RepoTab.Pulls -> overview?.pulls?.sumOf { it.pulls.size }
        RepoTab.Issues -> overview?.issues?.size
        RepoTab.Branches -> null
    }
    return if (count == null) tab.title else "${tab.title} $count"
}

/** The repository screen, stateless. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RepoScreen(
    repo: String,
    state: RepoUiState,
    actions: RepoActions,
    onBack: () -> Unit,
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenIssue: (IssueRef) -> Unit,
    onNewIssue: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val overview = (state.overview as? UiState.Loaded)?.data
    Column(modifier.fillMaxSize().background(colors.bg)) {
        BackTopBar(onBack = onBack, backDescription = "Back to the feed") {
            TitleStack(listOfNotNull(repo.substringBefore('/'), overview?.stars?.let { "★ $it" }).joinToString(" · ")) {
                Text(repo.substringAfter('/'), style = RostrumText.sheetTitle, color = colors.text, maxLines = 1)
            }
        }
        SegmentedToggle(
            options = RepoTab.entries.toList(),
            selected = state.tab,
            onSelect = actions::selectTab,
            label = { repoTabLabel(it, overview) },
            showCheck = false,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
        )
        Box(Modifier.weight(1f).fillMaxWidth()) {
            PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = actions::refresh, modifier = Modifier.fillMaxSize()) {
                when (state.tab) {
                    RepoTab.Pulls, RepoTab.Issues -> when (val loaded = state.overview) {
                        UiState.Loading -> LoadingView(label = "Loading $repo…")
                        is UiState.Error -> ErrorView(loaded.error, Modifier.padding(12.dp), title = "Couldn't load $repo")
                        is UiState.Loaded -> ItemsList(repo, state.tab, loaded.data, state.now, onOpenPullRequest, onOpenIssue)
                    }
                    RepoTab.Branches -> when (val branches = state.branches) {
                        null, UiState.Loading -> LoadingView(label = "Comparing branches…")
                        is UiState.Error -> ErrorView(branches.error, Modifier.padding(12.dp), title = "Couldn't load the branches", onRetry = actions::retryBranches)
                        is UiState.Loaded -> BranchTreeList(branches.data, actions, onOpenPullRequest)
                    }
                }
            }
            if (state.tab == RepoTab.Issues) NewIssueFab(onNewIssue, Modifier.align(Alignment.BottomEnd).padding(16.dp))
        }
    }
}

@Composable
private fun ItemsList(
    repo: String,
    tab: RepoTab,
    overview: RepoOverview,
    now: Instant,
    onOpenPullRequest: (PrRef) -> Unit,
    onOpenIssue: (IssueRef) -> Unit,
) {
    val colors = RostrumTheme.colors
    val rows = if (tab == RepoTab.Pulls) rowsOf(repo, overview.pulls) else overview.issues.map { ItemRow.Issue(it) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 88.dp)) {
        if (rows.isEmpty()) {
            item(key = "empty") {
                EmptyView(if (tab == RepoTab.Pulls) "No open pull requests" else "No open issues", body = "Pull to refresh $repo")
            }
        }
        rows.forEachIndexed { index, row ->
            item(key = row.key, contentType = row::class) {
                Box(Modifier.fillMaxWidth().cardSegment(SegmentPosition.of(index, rows.size), colors.surface, colors.border)) {
                    ItemRowContent(row, now, onOpenPullRequest, onOpenIssue)
                }
            }
        }
    }
}

@Composable
private fun BranchTreeList(tree: BranchTree, actions: RepoActions, onOpenPullRequest: (PrRef) -> Unit) {
    val colors = RostrumTheme.colors
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        item(key = "trunks") {
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(trunksSummary(tree.trunks), style = RostrumText.meta, color = colors.textMuted, modifier = Modifier.weight(1f))
                TextPillButton("Edit trunks", actions::openTrunkEditor)
            }
        }
        if (tree.rows.isEmpty()) item(key = "none") { EmptyView("No branches to show", body = "The repository has no commits yet.") }
        branchRows(tree, onOpenPullRequest)
    }
}

private fun LazyListScope.branchRows(tree: BranchTree, onOpenPullRequest: (PrRef) -> Unit) {
    tree.rows.forEachIndexed { index, row ->
        item(key = "branch-$index") { BranchRowView(row, tree, onOpenPullRequest) }
    }
}

@Composable
private fun BranchRowView(row: BranchRow, tree: BranchTree, onOpenPullRequest: (PrRef) -> Unit) {
    val colors = RostrumTheme.colors
    when (row) {
        is BranchRow.Trunk -> Row(
            Modifier.fillMaxWidth().heightIn(min = 44.dp).padding(top = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(row.name, style = RostrumText.mono12.copy(fontWeight = FontWeight.SemiBold), color = colors.text)
            Text(trunkDriftText(row.drift, tree.defaultBranch), style = RostrumText.meta, color = colors.textMuted, modifier = Modifier.weight(1f))
            Text("${row.pulls} PR${if (row.pulls == 1) "" else "s"}", style = RostrumText.mono12, color = colors.textMuted)
        }
        BranchRow.OtherBases -> Text(
            "Other bases",
            style = RostrumText.section,
            color = colors.textMuted,
            modifier = Modifier.padding(top = 16.dp, bottom = 4.dp),
        )
        is BranchRow.Base -> Row(Modifier.fillMaxWidth().heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(row.name, style = RostrumText.mono12, color = colors.textSecondary, modifier = Modifier.weight(1f))
            Text("${row.pulls} PR${if (row.pulls == 1) "" else "s"}", style = RostrumText.mono12, color = colors.textMuted)
        }
        is BranchRow.Pull -> {
            val pr = row.pull
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .then(if (pr != null) Modifier.clickable { onOpenPullRequest(pr.ref) } else Modifier)
                    .padding(start = (12 + 18 * row.depth).dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text("└", style = RostrumText.mono12, color = colors.textSubtle)
                Column(Modifier.weight(1f)) {
                    Text(pr?.title ?: row.head, style = RostrumText.label, color = colors.text, maxLines = 1)
                    Text(
                        buildString {
                            append("#${row.number} · ${row.head}")
                            row.drift?.let { append(" · ${driftText(it)}") }
                            row.note?.let { append(" · ${noteText(it)}") }
                        },
                        style = RostrumText.mono12,
                        color = colors.textMuted,
                        maxLines = 2,
                    )
                }
                row.stackLabel?.let { StatusChip(it, ColorRole.Accent) }
            }
        }
    }
}

/** Detect the trunks, or name them. */
@Composable
fun TrunkEditorContent(editor: TrunkEditor, existing: List<String>, actions: RepoActions) {
    val colors = RostrumTheme.colors
    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Trunks", style = RostrumText.sheetTitle, color = colors.text)
        Text(
            "Trunks are the long-lived branches pull requests are grouped under. Detection uses the default branch.",
            style = RostrumText.body,
            color = colors.textSecondary,
        )
        SegmentedToggle(
            options = listOf(true, false),
            selected = editor.detect,
            onSelect = actions::setTrunkDetect,
            label = { if (it) "Detect" else "Custom" },
            modifier = Modifier.fillMaxWidth(),
        )
        if (!editor.detect) {
            RostrumTextField(
                value = editor.text,
                onValueChange = actions::onTrunkTextChange,
                placeholder = "main, develop",
                accessibilityLabel = "Trunk branches",
                mono = true,
            )
        }
        if (existing.isNotEmpty()) Text("Found: ${existing.joinToString(", ")}", style = RostrumText.caption, color = colors.textMuted)
        (editor.save as? ActionState.Failed)?.let { FieldError(it.error.describe()) }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
            TextPillButton("Cancel", actions::closeTrunkEditor)
            Box(Modifier.width(4.dp))
            PrimaryButton("Save", actions::saveTrunks, busy = editor.save.running)
        }
    }
}
