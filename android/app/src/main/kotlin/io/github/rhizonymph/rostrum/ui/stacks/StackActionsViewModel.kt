package io.github.rhizonymph.rostrum.ui.stacks

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.StackJob
import io.github.rhizonymph.rostrum.data.model.StackMergeMethod
import io.github.rhizonymph.rostrum.data.model.StackPlanRequest
import io.github.rhizonymph.rostrum.data.model.StackSummary
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isPaired
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.Messages
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.common.toUiState
import io.github.rhizonymph.rostrum.ui.items.StackMenuEntry
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the stack sheets and the Arrange mode can ask for. */
interface StackActions {
    fun request(action: StackMenuEntry, repo: String, stack: StackSummary, members: List<PrSummary>)
    fun setMergeMethod(method: StackMergeMethod)
    fun toggleCandidate(number: Int)
    fun planExtend()
    fun moveUp(index: Int)
    fun moveDown(index: Int)
    fun setTrunk(trunk: String)
    fun planArrange()
    fun confirm()
    fun dismiss()
}

/**
 * Stack actions, run on the paired desktop. Each starts from a header's
 * menu (or the Arrange mode), asks before anything happens — for a rewrite,
 * the exact branches the desktop's dry run names, which are then the only
 * ones confirmed — and polls the started job until it finishes. Without a
 * desktop it says so and offers pairing.
 */
class StackActionsViewModel(
    private val backend: RostrumBackend,
    private val session: StateFlow<SessionState>,
    private val pollMillis: Long = POLL_MILLIS,
) : ViewModel(), StackActions {
    private val _flow = MutableStateFlow<StackFlow?>(null)
    val flow: StateFlow<StackFlow?> = _flow.asStateFlow()

    private val _selection = MutableStateFlow<ArrangeSelection?>(null)

    /** The Arrange mode's picks, while it is on. */
    val selection: StateFlow<ArrangeSelection?> = _selection.asStateFlow()

    val messages = Messages()
    private var polling: Job? = null

    private fun paired(action: String): Boolean {
        if (session.value.isPaired) return true
        _flow.value = StackFlow.NeedsDesktop(action)
        return false
    }

    override fun request(action: StackMenuEntry, repo: String, stack: StackSummary, members: List<PrSummary>) {
        if (!paired(action.label)) return
        val number = stack.number
        _flow.value = when (action) {
            StackMenuEntry.Merge -> number?.let { StackFlow.ConfirmMerge(repo, it, stack.title, members) }
            StackMenuEntry.Unstack -> number?.let { StackFlow.ConfirmUnstack(repo, it, stack.title) }
            StackMenuEntry.Make -> StackFlow.ConfirmMake(repo, members, stack.trunk)
            StackMenuEntry.AddTo -> number?.let { StackFlow.PickExtend(repo, it, stack.title) }
        }
        val pick = _flow.value as? StackFlow.PickExtend ?: return
        viewModelScope.launch {
            val candidates = backend.stackCandidates(pick.repo, pick.stack).toUiState()
            update<StackFlow.PickExtend> { it.copy(candidates = candidates) }
        }
    }

    private inline fun <reified T : StackFlow> update(transform: (T) -> StackFlow) {
        _flow.update { current -> if (current is T) transform(current) else current }
    }

    override fun setMergeMethod(method: StackMergeMethod) = update<StackFlow.ConfirmMerge> { it.copy(method = method) }

    /** Pick or unpick; the order of picking is the order on the stack. */
    override fun toggleCandidate(number: Int) = update<StackFlow.PickExtend> { pick ->
        pick.copy(chosen = if (number in pick.chosen) pick.chosen - number else pick.chosen + number, run = ActionState.Idle)
    }

    override fun planExtend() {
        val pick = _flow.value as? StackFlow.PickExtend ?: return
        if (pick.chosen.isEmpty() || pick.run.running) return
        update<StackFlow.PickExtend> { it.copy(run = ActionState.Running) }
        plan(StackPlanRequest.Extend(pick.repo, pick.stack, pick.chosen)) { error -> update<StackFlow.PickExtend> { it.copy(run = error) } }
    }

    // --- arrange ----------------------------------------------------------------------

    /** Pick pull requests of [repo] to arrange into a stack. */
    fun startArrange(repo: String) {
        if (!paired(ARRANGE)) return
        _selection.value = ArrangeSelection(repo)
    }

    fun toggleSelected(pr: PrSummary) {
        _selection.update { selection ->
            selection?.copy(picked = if (pr.number in selection.picked) selection.picked - pr.number else selection.picked + pr.number)
        }
    }

    fun cancelArrange() {
        _selection.value = null
    }

    /** On to ordering: [open] are the repository's pull requests to look the picks up in. */
    fun arrangeNext(open: List<PrSummary>) {
        val selection = _selection.value ?: return
        val members = selection.picked.mapNotNull { n -> open.firstOrNull { it.number == n } }
        if (members.size < 2) {
            messages.send("Pick at least two pull requests to arrange")
            return
        }
        _selection.value = null
        _flow.value = StackFlow.OrderArrange(selection.repo, members, trunk = members.first().baseRef)
        recheck()
    }

    private fun reorder(from: Int, to: Int) {
        update<StackFlow.OrderArrange> { order ->
            if (from !in order.members.indices || to !in order.members.indices) {
                order
            } else {
                val list = order.members.toMutableList()
                list.add(to, list.removeAt(from))
                order.copy(members = list, check = null, run = ActionState.Idle)
            }
        }
        recheck()
    }

    override fun moveUp(index: Int) = reorder(index, index - 1)

    override fun moveDown(index: Int) = reorder(index, index + 1)

    override fun setTrunk(trunk: String) {
        update<StackFlow.OrderArrange> { it.copy(trunk = trunk, check = null, run = ActionState.Idle) }
        recheck()
    }

    /** The phone's own check of the order, from the cached feed. */
    private fun recheck() {
        val order = _flow.value as? StackFlow.OrderArrange ?: return
        val request = StackPlanRequest.Arrange(order.repo, order.members.map { it.number }, order.trunk.trim())
        viewModelScope.launch {
            val check = backend.checkStackPlan(request)
            update<StackFlow.OrderArrange> { current ->
                if (current.members == order.members && current.trunk == order.trunk) {
                    current.copy(check = (check as? Outcome.Ok)?.value)
                } else {
                    current
                }
            }
        }
    }

    override fun planArrange() {
        val order = _flow.value as? StackFlow.OrderArrange ?: return
        if (order.run.running) return
        update<StackFlow.OrderArrange> { it.copy(run = ActionState.Running) }
        plan(StackPlanRequest.Arrange(order.repo, order.members.map { it.number }, order.trunk.trim())) { error ->
            update<StackFlow.OrderArrange> { it.copy(run = error) }
        }
    }

    /** Ask the desktop's dry run, then show exactly what it would rewrite. */
    private fun plan(request: StackPlanRequest, onError: (ActionState.Failed) -> Unit) {
        viewModelScope.launch {
            when (val plan = backend.planStackRewrite(request)) {
                is Outcome.Ok -> _flow.value = StackFlow.ConfirmRewrite(request, plan.value.rewrites)
                is Outcome.Err -> if (plan.error == BackendError.NotPaired) {
                    _flow.value = StackFlow.NeedsDesktop(if (request is StackPlanRequest.Arrange) ARRANGE else StackMenuEntry.AddTo.label)
                } else {
                    onError(ActionState.Failed(plan.error))
                }
            }
        }
    }

    // --- running ----------------------------------------------------------------------

    /** Run what the open confirmation asks for. */
    override fun confirm() {
        val current = _flow.value ?: return
        val call: suspend () -> Outcome<StackJob> = when (current) {
            is StackFlow.ConfirmMerge -> { { backend.mergeStack(current.repo, current.stack, current.method) } }
            is StackFlow.ConfirmUnstack -> { { backend.unstack(current.repo, current.stack) } }
            is StackFlow.ConfirmMake -> { { backend.makeStack(current.repo, current.members.map { it.number }, current.trunk) } }
            is StackFlow.ConfirmRewrite -> {
                val branches = current.rewrites.map { it.branch }
                when (val request = current.request) {
                    is StackPlanRequest.Arrange -> { { backend.arrangeStack(request.repo, request.prs, request.trunk, branches) } }
                    is StackPlanRequest.Extend -> { { backend.extendStack(request.repo, request.stack, request.prs, branches) } }
                }
            }
            else -> return
        }
        if (runOf(current).running) return
        _flow.value = withRun(current, ActionState.Running)
        viewModelScope.launch {
            when (val started = call()) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "stack_job_started", "kind" to started.value.kind, "repo" to started.value.repo)
                    follow(started.value)
                }
                is Outcome.Err -> refused(current, started.error)
            }
        }
    }

    private fun refused(current: StackFlow, error: BackendError) {
        RostrumLog.w(TAG, "stack_action_refused", "error" to error::class.simpleName)
        _flow.value = when {
            error == BackendError.NotPaired -> StackFlow.NeedsDesktop("Stack actions")
            // The desktop would rewrite other branches now: show those and ask again.
            error is BackendError.RewriteNotConfirmed && current is StackFlow.ConfirmRewrite ->
                current.copy(rewrites = error.branches, note = error.reason, run = ActionState.Idle)
            error is BackendError.RemoteApi && error.code == RemoteErrorCode.Busy -> {
                messages.send(error.describe())
                withRun(current, ActionState.Idle)
            }
            else -> withRun(current, ActionState.Failed(error))
        }
    }

    /** Show the job and poll it until it finishes. */
    private fun follow(job: StackJob) {
        _flow.value = StackFlow.Job(job)
        polling?.cancel()
        if (job.finished) return finished(job)
        polling = viewModelScope.launch {
            var current = job
            while (!current.finished) {
                delay(pollMillis)
                when (val polled = backend.stackJob(current.id)) {
                    is Outcome.Ok -> {
                        current = polled.value
                        if (_flow.value is StackFlow.Job) _flow.value = StackFlow.Job(current)
                    }
                    is Outcome.Err -> RostrumLog.w(TAG, "stack_job_poll_failed", "error" to polled.error::class.simpleName)
                }
            }
            finished(current)
        }
    }

    private fun finished(job: StackJob) {
        RostrumLog.i(TAG, "stack_job_finished", "kind" to job.kind, "state" to job.state::class.simpleName)
        messages.send(jobOutcome(job))
    }

    /** Close the sheet; a running job keeps going on the desktop (and still reports when done). */
    override fun dismiss() {
        val current = _flow.value
        if (current != null && current !is StackFlow.Job && runOf(current).running) return
        _flow.value = null
    }

    companion object {
        const val POLL_MILLIS = 1_000L
        const val ARRANGE = "Arrange"
        private const val TAG = "RostrumStacks"

        fun runOf(flow: StackFlow): ActionState = when (flow) {
            is StackFlow.ConfirmMerge -> flow.run
            is StackFlow.ConfirmUnstack -> flow.run
            is StackFlow.ConfirmMake -> flow.run
            is StackFlow.PickExtend -> flow.run
            is StackFlow.OrderArrange -> flow.run
            is StackFlow.ConfirmRewrite -> flow.run
            is StackFlow.NeedsDesktop, is StackFlow.Job -> ActionState.Idle
        }

        fun withRun(flow: StackFlow, run: ActionState): StackFlow = when (flow) {
            is StackFlow.ConfirmMerge -> flow.copy(run = run)
            is StackFlow.ConfirmUnstack -> flow.copy(run = run)
            is StackFlow.ConfirmMake -> flow.copy(run = run)
            is StackFlow.PickExtend -> flow.copy(run = run)
            is StackFlow.OrderArrange -> flow.copy(run = run)
            is StackFlow.ConfirmRewrite -> flow.copy(run = run)
            is StackFlow.NeedsDesktop, is StackFlow.Job -> flow
        }
    }
}
