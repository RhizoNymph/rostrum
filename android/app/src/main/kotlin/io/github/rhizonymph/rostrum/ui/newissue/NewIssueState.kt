package io.github.rhizonymph.rostrum.ui.newissue

import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.PickerKind
import io.github.rhizonymph.rostrum.ui.components.PickerState

/** The body field's two modes. */
enum class BodyMode { Write, Preview }

/** The new-issue form: a watched repository, a title, a markdown body, labels and assignees. */
data class NewIssueUiState(
    /** The profile's watched repositories, to pick from. */
    val repos: UiState<List<String>> = UiState.Loading,
    val repo: String? = null,
    val title: String = "",
    val body: String = "",
    val mode: BodyMode = BodyMode.Write,
    /** The body rendered by the core, while Preview is shown. */
    val preview: List<MdBlock> = emptyList(),
    val labels: Set<String> = emptySet(),
    val assignees: Set<String> = emptySet(),
    val picker: PickerState? = null,
    val repoPickerOpen: Boolean = false,
    val submit: ActionState = ActionState.Idle,
    /** The issue just opened; the route leaves for it. */
    val created: IssueRef? = null,
) {
    /** A repository and a non-blank title; nothing in flight. */
    val canSubmit: Boolean get() = repo != null && title.isNotBlank() && !submit.running

    val pickerSelection: Set<String>
        get() = when (picker?.kind) {
            PickerKind.Labels -> labels
            PickerKind.Assignees -> assignees
            null -> emptySet()
        }
}
