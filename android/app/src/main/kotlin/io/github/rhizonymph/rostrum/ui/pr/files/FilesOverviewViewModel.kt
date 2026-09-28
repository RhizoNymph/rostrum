package io.github.rhizonymph.rostrum.ui.pr.files

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.logErr
import io.github.rhizonymph.rostrum.data.model.FilesOverview
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.common.toUiState
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** The Files tab's landing view: summary, change map, ranked files. */
class FilesOverviewViewModel(
    private val backend: RostrumBackend,
    private val pr: PrRef,
) : ViewModel() {
    private val _state = MutableStateFlow<UiState<FilesOverview>>(UiState.Loading)
    val state: StateFlow<UiState<FilesOverview>> = _state.asStateFlow()

    private var job: Job? = null

    init {
        fetch(showLoading = true)
    }

    fun retry() = fetch(showLoading = true)

    /**
     * Re-fetch while keeping the current overview on screen (on returning
     * from a diff). Does nothing until the first load has succeeded.
     */
    fun refresh() {
        if (_state.value is UiState.Loaded) fetch(showLoading = false)
    }

    /** The file the Diff toggle opens: the largest change. */
    fun firstRankedFile(): Int? = _state.value.dataOrNull()?.ranked?.firstOrNull()?.fileIndex

    private fun fetch(showLoading: Boolean) {
        job?.cancel()
        if (showLoading) _state.value = UiState.Loading
        job = viewModelScope.launch {
            val outcome = backend.filesOverview(pr).logErr(TAG, "files_overview_failed", "pr" to pr)
            // A failed refresh keeps the overview already shown.
            if (outcome is Outcome.Err && _state.value is UiState.Loaded) return@launch
            _state.value = outcome.toUiState()
        }
    }

    private companion object {
        const val TAG = "RostrumFiles"
    }
}
