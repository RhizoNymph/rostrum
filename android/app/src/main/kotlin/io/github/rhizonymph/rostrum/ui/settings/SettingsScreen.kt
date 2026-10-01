package io.github.rhizonymph.rostrum.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.CloneInfo
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.components.ConfirmDialog
import io.github.rhizonymph.rostrum.ui.components.ErrorView
import io.github.rhizonymph.rostrum.ui.components.LoadingView
import io.github.rhizonymph.rostrum.ui.components.RadioVisual
import io.github.rhizonymph.rostrum.ui.components.ScreenHeader
import io.github.rhizonymph.rostrum.ui.components.TextPillButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/** The Settings tab, stateless: everything comes in through [state] and [actions]. */
@Composable
fun SettingsScreen(
    state: SettingsUiState,
    actions: SettingsActions,
    onOpenDesktop: () -> Unit,
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
    onCopySettings: () -> Unit = {},
) {
    val colors = RostrumTheme.colors
    var confirmSignOut by rememberSaveable { mutableStateOf(false) }
    var chooseInterval by rememberSaveable { mutableStateOf(false) }
    Column(modifier.fillMaxSize().background(colors.bg)) {
        ScreenHeader("Settings")
        when (val content = state.content) {
            UiState.Loading -> LoadingView(label = "Loading settings")
            is UiState.Error -> ErrorView(
                content.error,
                modifier = Modifier.padding(horizontal = 12.dp),
                title = "Couldn't load settings",
                onRetry = actions::retry,
            )
            is UiState.Loaded -> {
                val data = content.data
                Column(
                    Modifier
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .padding(horizontal = 12.dp),
                    verticalArrangement = Arrangement.spacedBy(20.dp),
                ) {
                    AccountCard(
                        account = data.account,
                        desktopName = (data.desktop as? DesktopSummary.Connected)?.machine?.name,
                        onSignOut = { confirmSignOut = true },
                        onUseDesktopToken = actions::refreshTokenFromDesktop,
                    )
                    RepositoriesSection(
                        repos = data.repos,
                        addRepo = state.addRepo,
                        removing = state.removing,
                        onInputChange = actions::onAddInputChange,
                        onAdd = actions::addRepo,
                        onRemove = actions::removeRepo,
                    )
                    DesktopSection(data.desktop, onOpenDesktop = onOpenDesktop, onPairDesktop = onPairDesktop, onCopySettings = onCopySettings)
                    SyncSection(
                        refreshIntervalSecs = data.refreshIntervalSecs,
                        notifyNewPullRequests = data.notifyNewPullRequests,
                        notifyReviewRequests = data.notifyReviewRequests,
                        onChooseInterval = { chooseInterval = true },
                        onNotifyNewChange = actions::setNotifyNewPullRequests,
                        onNotifyReviewsChange = actions::setNotifyReviewRequests,
                    )
                    Spacer(Modifier.height(12.dp))
                }
                if (chooseInterval) {
                    RefreshIntervalDialog(
                        current = data.refreshIntervalSecs,
                        onChoose = {
                            chooseInterval = false
                            actions.setRefreshInterval(it)
                        },
                        onDismiss = { chooseInterval = false },
                    )
                }
            }
        }
    }
    if (confirmSignOut) {
        ConfirmDialog(
            title = "Sign out of GitHub?",
            body = "Rostrum forgets the token on this phone. Your repositories and the desktop pairing are kept.",
            confirmLabel = "Sign out",
            destructive = true,
            onConfirm = {
                confirmSignOut = false
                actions.signOut()
            },
            onDismiss = { confirmSignOut = false },
        )
    }
}

@Composable
private fun RefreshIntervalDialog(current: Long, onChoose: (Long) -> Unit, onDismiss: () -> Unit) {
    val colors = RostrumTheme.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = colors.raised,
        titleContentColor = colors.text,
        shape = RoundedCornerShape(28.dp),
        title = { Text("Refresh while open", style = RostrumText.sheetTitle) },
        text = {
            Column {
                RefreshIntervalChoices.forEach { seconds ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .selectable(selected = seconds == current, role = Role.RadioButton, onClick = { onChoose(seconds) }),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        RadioVisual(seconds == current)
                        Text(refreshIntervalLabel(seconds), style = RostrumText.rowTitle, color = colors.text)
                    }
                }
            }
        },
        confirmButton = { TextPillButton("Cancel", onDismiss) },
    )
}

// --- previews ----------------------------------------------------------------

private val previewMachine = MachineInfo(
    name = "nymph-desk", version = "0.1.0", apiVersion = 1,
    clones = listOf(CloneInfo("RhizoNymph/rostrum", "~/Code/devtools/rostrum"), CloneInfo("zed-industries/zed", "~/Code/zed")),
    handlerConfigured = true, autostash = false,
)

private val previewContent = SettingsContent(
    account = AccountInfo(AccountViewer.Known("RhizoNymph"), "github.com"),
    repos = listOf(
        RepoRow("RhizoNymph/rostrum", RepoDetail.Clone("nymph-desk", "~/Code/devtools/rostrum")),
        RepoRow("zed-industries/zed", RepoDetail.Clone("nymph-desk", "~/Code/zed")),
        RepoRow("rust-lang/rust", RepoDetail.NoClone),
        RepoRow("tokio-rs/tokio", RepoDetail.HiddenEmpty),
        RepoRow("bevyengine/bevy", RepoDetail.HiddenEmpty),
    ),
    desktop = DesktopSummary.Connected(previewMachine),
    refreshIntervalSecs = 60,
    notifyNewPullRequests = true,
    notifyReviewRequests = true,
)

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun SettingsPreview() {
    RostrumTheme {
        SettingsScreen(
            state = SettingsUiState(
                content = UiState.Loaded(previewContent),
                addRepo = AddRepoState(input = "rust-lang/rust", error = BackendError.DuplicateRepo("rust-lang/rust")),
            ),
            actions = NoSettingsActions,
            onOpenDesktop = {},
            onPairDesktop = {},
        )
    }
}

@Preview(widthDp = 412, heightDp = 600)
@Composable
private fun SettingsErrorPreview() {
    RostrumTheme {
        SettingsScreen(
            state = SettingsUiState(content = UiState.Error(BackendError.Storage("config.json is unreadable"))),
            actions = NoSettingsActions,
            onOpenDesktop = {},
            onPairDesktop = {},
        )
    }
}
