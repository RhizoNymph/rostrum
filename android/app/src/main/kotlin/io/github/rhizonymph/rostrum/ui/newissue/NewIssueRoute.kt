package io.github.rhizonymph.rostrum.ui.newissue

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.components.PickerSheet

/**
 * The new-issue form for the active profile. [repo] presets the repository
 * (from its screen); [onCreated] leaves for the new issue.
 */
@Composable
fun NewIssueRoute(repo: String?, onBack: () -> Unit, onCreated: (IssueRef) -> Unit, modifier: Modifier = Modifier) {
    val viewModel = profileViewModel(key = "new-issue:${repo.orEmpty()}") { container, profile ->
        NewIssueViewModel(profile.backend, repo, container.appMessages)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val created by rememberUpdatedState(onCreated)
    CollectMessages(viewModel.messages.flow)
    LaunchedEffect(state.created) {
        state.created?.let { created(it) }
    }
    NewIssueScreen(state, viewModel, onBack, modifier)
    if (state.repoPickerOpen) RepoPickerSheet(state, viewModel)
    state.picker?.let { picker ->
        PickerSheet(
            state = picker,
            selected = state.pickerSelection,
            onToggle = viewModel::toggle,
            onRetry = viewModel::retryPicker,
            onDismiss = viewModel::closePicker,
        )
    }
}
