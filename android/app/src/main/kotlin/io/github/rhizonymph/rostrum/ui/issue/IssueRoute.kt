package io.github.rhizonymph.rostrum.ui.issue

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.ui.common.CollectMessages
import io.github.rhizonymph.rostrum.ui.common.profileViewModel
import io.github.rhizonymph.rostrum.ui.components.PickerSheet

/** One issue of the active profile, wired into the navigation graph. */
@Composable
fun IssueRoute(issue: IssueRef, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val viewModel = profileViewModel(key = "issue:$issue") { container, profile ->
        IssueViewModel(profile.backend, issue, container.clock)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    CollectMessages(viewModel.messages.flow)
    IssueScreen(issue, state, viewModel, onBack, modifier)
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
