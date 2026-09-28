package io.github.rhizonymph.rostrum.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.ui.components.Avatar
import io.github.rhizonymph.rostrum.ui.components.CardDivider
import io.github.rhizonymph.rostrum.ui.components.ChevronRow
import io.github.rhizonymph.rostrum.ui.components.IconTile
import io.github.rhizonymph.rostrum.ui.components.RostrumCard
import io.github.rhizonymph.rostrum.ui.components.RostrumIconButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.SectionHeader
import io.github.rhizonymph.rostrum.ui.components.StatusDot
import io.github.rhizonymph.rostrum.ui.components.SwitchRow
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.components.TonalButton
import io.github.rhizonymph.rostrum.ui.format.splitRepo
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** Avatar, login, host line, Sign out; and, when paired, a way to take the desktop's token. */
@Composable
fun AccountCard(
    account: AccountInfo,
    desktopName: String?,
    onSignOut: () -> Unit,
    onUseDesktopToken: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    RostrumCard(modifier.fillMaxWidth()) {
        Row(
            Modifier.padding(start = 14.dp, end = 6.dp, top = 14.dp, bottom = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            val login = (account.viewer as? AccountViewer.Known)?.login
            Avatar(login ?: account.host, size = 40.dp)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    login ?: "Signed in",
                    style = RostrumText.rowTitle.copy(fontWeight = FontWeight.SemiBold),
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(accountSubline(account.host), style = RostrumText.caption, color = colors.textMuted)
                if (account.viewer is AccountViewer.Unknown) {
                    Text("Couldn't reach GitHub to check who you are", style = RostrumText.caption, color = colors.warningText)
                }
            }
            TextPillButton("Sign out", onSignOut)
        }
        if (desktopName != null) {
            CardDivider()
            TextPillButton(
                "Use $desktopName's GitHub token",
                onUseDesktopToken,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
    }
}

/** The add field, its inline error, and the list of watched repositories. */
@Composable
fun RepositoriesSection(
    repos: List<RepoRow>,
    addRepo: AddRepoState,
    removing: Set<String>,
    onInputChange: (String) -> Unit,
    onAdd: () -> Unit,
    onRemove: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader("Repositories", trailing = repos.size.toString())
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Add a repository", style = RostrumText.caption, color = colors.textMuted, modifier = Modifier.padding(horizontal = 4.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                RostrumTextField(
                    value = addRepo.input,
                    onValueChange = onInputChange,
                    modifier = Modifier.weight(1f),
                    placeholder = "owner/name or GitHub URL",
                    accessibilityLabel = "Add a repository",
                    mono = true,
                    isError = addRepo.error != null,
                    errorText = addRepo.error?.let { addRepoError(it).let { e -> (e.code ?: "") + e.rest } },
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.None,
                        autoCorrectEnabled = false,
                        keyboardType = KeyboardType.Uri,
                        imeAction = ImeAction.Done,
                    ),
                    keyboardActions = KeyboardActions(onDone = { onAdd() }),
                )
                TonalButton(
                    "Add",
                    onAdd,
                    enabled = addRepo.input.isNotBlank(),
                    busy = addRepo.running,
                    height = 48.dp,
                )
            }
            addRepo.error?.let { RepoInputError(addRepoError(it)) }
        }
        if (repos.isNotEmpty()) {
            RostrumCard(Modifier.fillMaxWidth()) {
                repos.forEachIndexed { index, row ->
                    if (index > 0) CardDivider()
                    RepoListRow(row, removing = row.repo in removing, onRemove = { onRemove(row.repo) })
                }
            }
        }
    }
}

@Composable
private fun RepoInputError(error: InlineError) {
    val colors = RostrumTheme.colors
    Row(
        Modifier.padding(horizontal = 4.dp).semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(RostrumIcons.Alert, contentDescription = null, tint = colors.dangerText, modifier = Modifier.size(14.dp))
        Text(
            buildAnnotatedString {
                if (error.code != null) withStyle(SpanStyle(fontFamily = RostrumFonts.Mono)) { append(error.code) }
                append(error.rest)
            },
            style = RostrumText.caption,
            color = colors.dangerText,
        )
    }
}

@Composable
private fun RepoListRow(row: RepoRow, removing: Boolean, onRemove: () -> Unit) {
    val colors = RostrumTheme.colors
    val (owner, name) = splitRepo(row.repo)
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).padding(start = 14.dp, end = 2.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(
                buildAnnotatedString {
                    withStyle(SpanStyle(color = colors.textMuted)) { append(owner) }
                    withStyle(SpanStyle(color = colors.text, fontWeight = FontWeight.SemiBold)) { append(name) }
                },
                style = RostrumText.label.copy(fontWeight = FontWeight.Normal),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            row.detail.subline()?.let {
                Text(
                    it,
                    style = RostrumText.mono12,
                    color = if (row.detail == RepoDetail.HiddenEmpty) colors.textSubtle else colors.textMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        RostrumIconButton(
            icon = RostrumIcons.Close,
            contentDescription = "Remove ${row.repo}",
            onClick = onRemove,
            enabled = !removing,
            tint = colors.textMuted,
            iconSize = 18.dp,
        )
    }
}

/** The paired desktop, or an invitation to pair one. */
@Composable
fun DesktopSection(
    desktop: DesktopSummary,
    onOpenDesktop: () -> Unit,
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
    onCopySettings: () -> Unit = {},
) {
    val colors = RostrumTheme.colors
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader("Desktop")
        RostrumCard(Modifier.fillMaxWidth()) {
            ChevronRow(onClick = if (desktop is DesktopSummary.NotPaired) onPairDesktop else onOpenDesktop) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    IconTile(size = 36.dp) {
                        Icon(RostrumIcons.Desktop, contentDescription = null, tint = colors.accentText, modifier = Modifier.size(20.dp))
                    }
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        val (title, status, dot) = when (desktop) {
                            DesktopSummary.NotPaired -> Triple("Pair with your desktop", "Not paired", colors.textSubtle)
                            is DesktopSummary.Connected -> Triple(desktop.machine.name, "Connected", colors.success)
                            is DesktopSummary.Unreachable -> Triple("Paired desktop", "Unreachable", colors.danger)
                        }
                        Text(title, style = RostrumText.rowTitle, color = colors.text)
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            StatusDot(dot)
                            Text(status, style = RostrumText.caption, color = colors.textMuted)
                        }
                    }
                }
            }
            if (desktop is DesktopSummary.Connected) {
                CardDivider()
                ChevronRow(onClick = onCopySettings) {
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text("Copy settings from ${desktop.machine.name}", style = RostrumText.rowTitle, color = colors.text)
                        Text(
                            "Repositories, feed filters and stash default",
                            style = RostrumText.caption,
                            color = colors.textMuted,
                        )
                    }
                }
            }
        }
    }
}

/** Refresh cadence, the background check, and the two notification switches. */
@Composable
fun SyncSection(
    refreshIntervalSecs: Long,
    notifyNewPullRequests: Boolean,
    notifyReviewRequests: Boolean,
    onChooseInterval: () -> Unit,
    onNotifyNewChange: (Boolean) -> Unit,
    onNotifyReviewsChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionHeader("Sync")
        RostrumCard(Modifier.fillMaxWidth()) {
            ValueRow("Refresh while open", refreshIntervalLabel(refreshIntervalSecs), onClick = onChooseInterval)
            CardDivider()
            ValueRow("Background check", "every 15 min", onClick = null)
            CardDivider()
            SwitchRow("Notify on new pull requests", notifyNewPullRequests, onNotifyNewChange)
            CardDivider()
            SwitchRow("Notify when your review is requested", notifyReviewRequests, onNotifyReviewsChange)
        }
    }
}

@Composable
private fun ValueRow(title: String, value: String, onClick: (() -> Unit)?) {
    val colors = RostrumTheme.colors
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(title, style = RostrumText.rowTitle, color = colors.text, modifier = Modifier.weight(1f))
        Text(value, style = RostrumText.mono13, color = colors.textMuted)
    }
}
