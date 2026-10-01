package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.format.splitRepo
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme
import java.time.Instant

/** One lazy item of a repository card. */
private sealed interface Segment {
    val key: String

    data class Header(val section: RepoSection) : Segment {
        override val key get() = "header:${section.repo}"
    }

    data class StaleNotice(val repo: String, val reason: String) : Segment {
        override val key get() = "stale:$repo"
    }

    data class Pull(val pr: PrSummary) : Segment {
        override val key get() = "pr:${pr.repo}#${pr.number}"
    }

    data class Body(val section: RepoSection) : Segment {
        override val key get() = "body:${section.repo}"
    }
}

private fun segmentsOf(section: RepoSection): List<Segment> = buildList {
    add(Segment.Header(section))
    when (val body = section.body) {
        RepoBody.Collapsed -> Unit
        is RepoBody.Pulls -> {
            (section.load as? RepoLoad.Failed)?.let { add(Segment.StaleNotice(section.repo, it.reason)) }
            body.pulls.forEach { add(Segment.Pull(it)) }
        }
        RepoBody.Loading, RepoBody.Empty, is RepoBody.Failed -> add(Segment.Body(section))
    }
}

/**
 * The loaded feed: each repository a card (drawn as segments so rows stay
 * lazy), then the "N empty repositories hidden" footer.
 */
@Composable
fun FeedList(
    snapshot: FeedSnapshot,
    now: Instant,
    authProblem: BackendError?,
    actions: FeedActions,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 8.dp),
    ) {
        if (authProblem != null) {
            item(key = "auth") { AuthBanner(authProblem, actions.signOut, Modifier.padding(bottom = 12.dp)) }
        }
        if (snapshot.repos.isEmpty()) {
            item(key = "empty") {
                if (snapshot.hiddenEmptyRepos == 0) {
                    EmptyView("No repositories yet", body = "Add repositories in Settings")
                } else {
                    EmptyView("Nothing to show", body = "Every repository is empty or filtered out")
                }
            }
        }
        snapshot.repos.forEachIndexed { cardIndex, section ->
            val segments = segmentsOf(section)
            segments.forEachIndexed { index, segment ->
                item(key = segment.key, contentType = segment::class) {
                    val position = SegmentPosition.of(index, segments.size)
                    val gap = if (index == 0 && cardIndex > 0) 12.dp else 0.dp
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = gap)
                            .cardSegment(position, colors.surface, colors.border),
                    ) {
                        SegmentContent(segment, now, actions)
                    }
                }
            }
        }
        if (snapshot.hiddenEmptyRepos > 0) {
            footer(snapshot.hiddenEmptyRepos)
        }
    }
}

private fun LazyListScope.footer(hidden: Int) {
    item(key = "hidden") {
        Text(
            hiddenReposText(hidden),
            style = RostrumText.caption,
            color = RostrumTheme.colors.textSubtle,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(top = 14.dp, bottom = 8.dp),
        )
    }
}

@Composable
private fun SegmentContent(segment: Segment, now: Instant, actions: FeedActions) {
    when (segment) {
        is Segment.Header -> RepoHeader(segment.section, onToggle = { actions.toggleCollapsed(segment.section.repo) })
        is Segment.StaleNotice -> StaleNotice(segment.reason, onRetry = { actions.retryRepo(segment.repo) })
        is Segment.Pull -> PrRow(segment.pr, now, onClick = { actions.openPullRequest(segment.pr.ref) })
        is Segment.Body -> RepoBodyRow(segment.section, onRetry = { actions.retryRepo(segment.section.repo) })
    }
}

/** 50dp: letter tile, `owner/` + **name**, visible count, collapse chevron. */
@Composable
private fun RepoHeader(section: RepoSection, onToggle: () -> Unit) {
    val colors = RostrumTheme.colors
    val (owner, name) = splitRepo(section.repo)
    Row(
        modifier = Modifier.fillMaxWidth().height(50.dp).padding(start = 14.dp, end = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier.size(22.dp).clip(RoundedCornerShape(6.dp)).background(colors.tonal),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                repoInitial(section.repo),
                style = RostrumText.mono11.copy(fontWeight = FontWeight.SemiBold),
                color = colors.accentText,
            )
        }
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = colors.textMuted)) { append(owner) }
                withStyle(SpanStyle(color = colors.text, fontWeight = FontWeight.SemiBold)) { append(name) }
            },
            style = RostrumText.label.copy(fontWeight = FontWeight.Normal),
            maxLines = 1,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        if (section.load == RepoLoad.Loading && section.body !is RepoBody.Loading) {
            CircularProgressIndicator(color = colors.textMuted, strokeWidth = 1.5.dp, modifier = Modifier.size(12.dp))
        }
        Text(section.visibleCount.toString(), style = RostrumText.mono12, color = colors.textMuted)
        RostrumIconButton(
            icon = if (section.collapsed) RostrumIcons.ChevronRight else RostrumIcons.ChevronDown,
            contentDescription = if (section.collapsed) "Expand ${section.repo}" else "Collapse ${section.repo}",
            onClick = onToggle,
            tint = colors.textMuted,
            iconSize = 18.dp,
        )
    }
}

@Composable
private fun RepoBodyRow(section: RepoSection, onRetry: () -> Unit) {
    val colors = RostrumTheme.colors
    when (val body = section.body) {
        RepoBody.Loading -> Row(
            Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            CircularProgressIndicator(color = colors.accent, strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
            Text("Loading pull requests…", style = RostrumText.meta, color = colors.textMuted)
        }
        is RepoBody.Failed -> Column(
            Modifier.fillMaxWidth().padding(start = 14.dp, end = 4.dp, top = 12.dp, bottom = 4.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.dangerText, modifier = Modifier.size(16.dp))
                Text("Couldn't load: ${body.reason}", style = RostrumText.meta, color = colors.textSecondary)
            }
            TextPillButton("Retry", onRetry)
        }
        RepoBody.Empty -> Text(
            emptyBodyText(section),
            style = RostrumText.meta,
            color = colors.textMuted,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 14.dp),
        )
        RepoBody.Collapsed, is RepoBody.Pulls -> Unit
    }
}

/** A repository whose last refresh failed but still shows its older pull requests. */
@Composable
private fun StaleNotice(reason: String, onRetry: () -> Unit) {
    val colors = RostrumTheme.colors
    Row(
        Modifier.fillMaxWidth().padding(start = 14.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.warningText, modifier = Modifier.size(16.dp))
        Text(
            "Couldn't refresh: $reason",
            style = RostrumText.caption,
            color = colors.textMuted,
            maxLines = 2,
            modifier = Modifier.weight(1f),
        )
        TextPillButton("Retry", onRetry)
    }
}

/** GitHub rejected the token: say so and offer signing in again. */
@Composable
fun AuthBanner(error: BackendError, onSignOut: () -> Unit, modifier: Modifier = Modifier) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 14.dp, bottom = 6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.dangerText, modifier = Modifier.size(18.dp))
                Text("Signed out of GitHub", style = RostrumText.rowTitle, color = colors.text)
            }
            Text(error.describe(), style = RostrumText.meta, color = colors.textMuted)
            TextPillButton("Sign in again", onSignOut, style = RostrumText.button.copy(fontFamily = RostrumFonts.Sans))
        }
    }
}
