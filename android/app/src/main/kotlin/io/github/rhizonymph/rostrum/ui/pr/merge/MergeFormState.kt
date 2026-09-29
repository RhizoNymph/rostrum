package io.github.rhizonymph.rostrum.ui.pr.merge

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.MergeMethod
import io.github.rhizonymph.rostrum.data.model.PullHeader
import io.github.rhizonymph.rostrum.ui.pr.common.defaultCommit

/**
 * The merge sheet: its method, the commit text GitHub will write, and the
 * submission in flight. Only exists while the sheet is open.
 */
data class MergeFormState(
    val method: MergeMethod,
    val title: String,
    val message: String,
    val submitting: Boolean = false,
    val error: BackendError? = null,
) {
    /** Rebase replays the commits as they are; it writes no commit of its own. */
    val hasCommitText: Boolean get() = method != MergeMethod.Rebase

    companion object {
        /** A fresh form for [method], pre-filled with GitHub's defaults. */
        fun forMethod(method: MergeMethod, header: PullHeader): MergeFormState {
            val commit = defaultCommit(method, header)
            return MergeFormState(method, commit?.title.orEmpty(), commit?.message.orEmpty())
        }
    }
}
