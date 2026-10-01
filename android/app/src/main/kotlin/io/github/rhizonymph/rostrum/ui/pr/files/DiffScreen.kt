package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Icon
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.model.ChangedFile
import io.github.rhizonymph.rostrum.data.model.DiffAvailability
import io.github.rhizonymph.rostrum.data.model.FileDiffBody
import io.github.rhizonymph.rostrum.data.model.PendingReview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.StatusChip
import io.github.rhizonymph.rostrum.ui.format.additionsText
import io.github.rhizonymph.rostrum.ui.format.deletionsText
import io.github.rhizonymph.rostrum.ui.format.directoryOf
import io.github.rhizonymph.rostrum.ui.format.fileName
import io.github.rhizonymph.rostrum.ui.format.shortSha
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.review.ReviewLabels
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** The diff screen's chrome callbacks; the rows report through [body]. */
class DiffScreenActions(
    val onBack: () -> Unit,
    val onPrevious: () -> Unit,
    val onNext: () -> Unit,
    val onToggleSoftWrap: () -> Unit,
    val onToggleViewed: () -> Unit,
    val onRetry: () -> Unit,
    val onFinishReview: () -> Unit,
    val body: DiffBodyActions,
)

/** One file's diff: header with file navigation, sub-bar, rows, hint, pending-review bar. */
@Composable
fun DiffScreen(state: FileDiffState, now: Instant, actions: DiffScreenActions, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    Column(modifier.fillMaxSize().background(colors.bg)) {
        DiffTopBar(state, actions)
        state.file?.let { DiffSubBar(it, state.isViewed, actions.onToggleViewed) }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when (val diff = state.diff) {
                UiState.Loading -> LoadingView(Modifier.padding(top = 32.dp), label = "Loading diff…")
                is UiState.Error -> ErrorView(diff.error, Modifier.padding(16.dp), title = "Couldn't load this diff", onRetry = actions.onRetry)
                is UiState.Loaded -> when (val body = diff.data.body) {
                    // Keyed by file, so scroll positions start over on previous/next
                    // but survive reloads of the same file (a new draft, a reply).
                    is FileDiffBody.Rows -> key(diff.data.file.index) {
                        DiffBody(
                            rows = body.rows,
                            softWrap = state.softWrap,
                            selectedRows = state.selectedRows,
                            reply = state.reply,
                            now = now,
                            actions = actions.body,
                        )
                    }
                    FileDiffBody.Unavailable -> UnavailableDiff(diff.data.file)
                }
            }
        }
        if (state.rows.isNotEmpty()) HintStrip()
        state.pending?.takeIf { it.drafts.isNotEmpty() }?.let { PendingBar(it, actions.onFinishReview) }
    }
}

@Composable
private fun DiffTopBar(state: FileDiffState, actions: DiffScreenActions) {
    val colors = RostrumTheme.colors
    val path = state.file?.path.orEmpty()
    val nav = state.nav
    BackTopBar(
        onBack = actions.onBack,
        backDescription = "Back to files",
        actions = {
            if (nav != null) {
                RostrumIconButton(
                    RostrumIcons.ChevronLeft, "Previous file", actions.onPrevious,
                    enabled = nav.hasPrevious, tint = colors.textSecondary, iconSize = 20.dp,
                )
                Text(nav.label, style = RostrumText.mono12, color = colors.textMuted)
                RostrumIconButton(
                    RostrumIcons.ChevronRight, "Next file", actions.onNext,
                    enabled = nav.hasNext, tint = colors.textSecondary, iconSize = 20.dp,
                )
            }
            IconToggleButton(
                checked = state.softWrap,
                onCheckedChange = { actions.onToggleSoftWrap() },
                modifier = Modifier.size(48.dp),
                colors = IconButtonDefaults.iconToggleButtonColors(
                    contentColor = colors.textSecondary,
                    checkedContentColor = colors.accentText,
                ),
            ) {
                Icon(RostrumIcons.SoftWrap, contentDescription = "Soft wrap", modifier = Modifier.size(22.dp))
            }
        },
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            val directory = directoryOf(path)
            if (directory.isNotEmpty()) {
                Text(directory, style = RostrumText.mono12, color = colors.textMuted, maxLines = 1, overflow = TextOverflow.StartEllipsis)
            }
            Text(fileName(path), style = RostrumText.monoStrong15, color = colors.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun DiffSubBar(file: ChangedFile, viewed: Boolean, onToggleViewed: () -> Unit) {
    val colors = RostrumTheme.colors
    val chip = statusChip(file.status)
    Column(Modifier.fillMaxWidth().background(colors.surface)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
        Row(
            Modifier.fillMaxWidth().height(48.dp).padding(start = 16.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            StatusChip(chip.text, chip.role)
            Text(
                buildAnnotatedString {
                    withStyle(RostrumText.mono12.toSpanStyle().copy(color = colors.successText)) { append(additionsText(file.additions)) }
                    append(" ")
                    val deletionsColor = if (file.deletions == 0) colors.textSubtle else colors.dangerText
                    withStyle(RostrumText.mono12.toSpanStyle().copy(color = deletionsColor)) { append(deletionsText(file.deletions)) }
                },
                style = RostrumText.mono12,
            )
            Spacer(Modifier.weight(1f))
            Row(
                modifier = Modifier
                    .height(48.dp)
                    .toggleable(value = viewed, role = Role.Checkbox, onValueChange = { onToggleViewed() })
                    .padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(
                    if (viewed) RostrumIcons.CheckBold else RostrumIcons.Eye,
                    contentDescription = null,
                    tint = colors.accentText,
                    modifier = Modifier.size(16.dp),
                )
                Text("Viewed", style = RostrumText.button.copy(fontSize = RostrumText.meta.fontSize), color = colors.accentText)
            }
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
    }
}

@Composable
private fun UnavailableDiff(file: ChangedFile) {
    val (title, body) = when (file.availability) {
        DiffAvailability.Binary -> "Binary file" to "GitHub doesn't send a diff for binary files."
        DiffAvailability.TooLarge -> "Diff too large" to "GitHub withheld this file's patch because it is too large. Open it on GitHub to read it."
        DiffAvailability.Unparseable -> "Couldn't read this diff" to "GitHub sent a patch Rostrum couldn't parse."
        DiffAvailability.NoTextChanges -> "No line changes" to (
            file.previousPath?.let { "Renamed from $it, with no changes to its lines." }
                ?: "This file was renamed, copied or had its mode changed, with no changes to its lines."
            )
        DiffAvailability.Text -> "Nothing to show" to "This file has no diff lines."
    }
    EmptyView(title, body = body)
}

@Composable
private fun HintStrip() {
    val colors = RostrumTheme.colors
    Column(Modifier.fillMaxWidth().background(colors.bg)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
        Row(
            Modifier.fillMaxWidth().height(28.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(RostrumIcons.Tap, contentDescription = null, tint = colors.textSubtle, modifier = Modifier.size(14.dp))
            Text("Tap a line number to comment · hold and drag for a range", style = RostrumText.caption, color = colors.textSubtle, maxLines = 1)
        }
    }
}

@Composable
private fun PendingBar(pending: PendingReview, onFinishReview: () -> Unit) {
    val colors = RostrumTheme.colors
    Column(Modifier.fillMaxWidth().background(colors.surface)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.border))
        Row(
            Modifier.fillMaxWidth().height(72.dp).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}, verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(ReviewLabels.pendingCount(pending.drafts.size), style = RostrumText.label.copy(fontWeight = RostrumText.button.fontWeight), color = colors.text)
                if (pending.stale) {
                    Text(
                        "head moved since you drafted",
                        style = RostrumText.mono12,
                        color = colors.dangerText,
                        modifier = Modifier.semantics { contentDescription = "New commits arrived since you drafted these comments" },
                    )
                } else {
                    pending.draftedAgainst?.let {
                        Text("drafted on ${shortSha(it)}", style = RostrumText.mono12, color = colors.textMuted)
                    }
                }
            }
            PrimaryButton("Finish review", onFinishReview)
        }
    }
}

// --- previews --------------------------------------------------------------------

private fun previewActions() = DiffScreenActions(
    onBack = {}, onPrevious = {}, onNext = {}, onToggleSoftWrap = {}, onToggleViewed = {}, onRetry = {}, onFinishReview = {},
    body = DiffBodyActions({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}),
)

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun DiffScreenPreview() {
    val pr = PrRef("RhizoNymph/rostrum", 10)
    val diff = PreviewData.sample { fileDiff(pr, 1) }
    val pending = PreviewData.sample { pendingReview(pr) }
    val overview = PreviewData.sample { filesOverview(pr) }
    val state = FileDiffState(
        fileIndex = 1,
        nav = FileNav.of(overview.ranked.map { it.fileIndex }, 1),
        file = diff.file,
        diff = UiState.Loaded(diff),
        pending = pending,
    )
    RostrumTheme { DiffScreen(state, PreviewData.clock.instant(), previewActions()) }
}

@Preview(widthDp = 412, heightDp = 500)
@Composable
private fun DiffScreenLoadingPreview() {
    RostrumTheme { DiffScreen(FileDiffState(fileIndex = 0), PreviewData.clock.instant(), previewActions()) }
}
