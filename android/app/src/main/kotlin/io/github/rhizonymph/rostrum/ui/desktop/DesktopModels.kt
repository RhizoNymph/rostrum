package io.github.rhizonymph.rostrum.ui.desktop

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.JobOutcome
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncEntryState
import io.github.rhizonymph.rostrum.data.model.SyncRun
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.format.countLabel
import io.github.rhizonymph.rostrum.ui.format.relativeAgo
import java.time.Instant

/** What the Desktop tab shows as a whole. */
sealed interface DesktopPage {
    data object Loading : DesktopPage

    data object NotPaired : DesktopPage

    /** Paired, but the desktop did not answer (or refused this phone). */
    data class Unreachable(val error: BackendError) : DesktopPage

    data class Loaded(val content: DesktopContent) : DesktopPage
}

/** "Sync all" as this phone sees it. */
sealed interface SyncActivity {
    data object Idle : SyncActivity

    /** Asked the desktop to start [op]; no answer yet. */
    data class Starting(val op: SyncAllOp) : SyncActivity

    /** Running on the desktop; polled for progress. */
    data class Running(val run: SyncRun) : SyncActivity
}

data class DesktopContent(
    val machine: MachineInfo,
    /** Loaded separately, so a failure here leaves the rest usable. */
    val handoffs: UiState<List<HandoffSession>>,
    /** The "Stash local changes" switch (the phone's setting). */
    val autostash: Boolean,
    val sync: SyncActivity,
    /** The most recent finished "sync all". */
    val lastRun: SyncRun?,
    /** Handoff sessions whose abort is in flight, by session name. */
    val aborting: Set<String> = emptySet(),
)

data class DesktopState(
    val page: DesktopPage = DesktopPage.Loading,
    /** The desktop could not be told about an unpair; offer to forget it here only. */
    val unpairFailure: BackendError? = null,
    val unpairing: Boolean = false,
)

val SyncAllOp.title: String
    get() = when (this) {
        SyncAllOp.Pull -> "Pull all"
        SyncAllOp.MergeBase -> "Merge base into all"
        SyncAllOp.RebaseBase -> "Rebase all onto base"
    }

/** The git command each op runs, shown in mono next to it. */
val SyncAllOp.hint: String
    get() = when (this) {
        SyncAllOp.Pull -> "pull --rebase"
        SyncAllOp.MergeBase -> "merge origin/<base>"
        SyncAllOp.RebaseBase -> "rebase origin/<base>"
    }

/** A finished run's title, from the operation it ran. */
fun runTitle(op: LocalOp): String = when (op) {
    LocalOp.PullRebase -> "Pull all"
    LocalOp.MergeRemote -> "Merge remote into all"
    LocalOp.MergeBase -> "Merge base into all"
    LocalOp.RebaseBase -> "Rebase all onto base"
}

/** `Connected · 2 clones`. */
fun machineSummary(machine: MachineInfo): String =
    "Connected · ${countLabel(machine.clones.size, "clone")}"

/** `#10 feat/diff-overview · started 3m ago`; parts the desktop did not report are left out. */
fun handoffMeta(session: HandoffSession, now: Instant): String {
    val what = listOfNotNull(session.number?.let { "#$it" }, session.headRef).joinToString(" ").ifBlank { null }
    val started = session.startedAt?.let { "started ${relativeAgo(it, now)}" }
    return listOfNotNull(what, started).joinToString(" · ")
}

fun waitingLabel(count: Int): String = "$count waiting"

/** One pull request's line in the last run. */
data class SyncEntryView(
    val pr: PrRef,
    val headRef: String,
    val chip: Chip?,
    val detail: String,
)

/** A finished run split into what needs a look and what simply updated. */
data class LastRunView(
    val title: String,
    val summary: String,
    val finishedAt: Instant?,
    /** Handed off, refused, conflicted, failed. */
    val attention: List<SyncEntryView>,
    /** Updated, already up to date, or skipped. */
    val settled: List<SyncEntryView>,
    val updatedCount: Int,
)

fun lastRunView(run: SyncRun): LastRunView {
    val attention = mutableListOf<SyncEntryView>()
    val settled = mutableListOf<SyncEntryView>()
    for (entry in run.entries) {
        val result = (entry.state as? SyncEntryState.Done)?.result ?: continue
        val view = SyncEntryView(PrRef(entry.repo, entry.number), entry.headRef, result.chip, result.detail)
        if (result.outcome.needsAttention()) attention += view else settled += view
    }
    return LastRunView(
        title = runTitle(run.op),
        summary = run.progressText,
        finishedAt = run.finishedAt,
        attention = attention,
        settled = settled,
        updatedCount = run.summary.updated,
    )
}

/** Outcomes someone should look at: handed off, refused, conflicted, failed. */
fun JobOutcome.needsAttention(): Boolean = when (this) {
    is JobOutcome.HandedOff, is JobOutcome.Refused, is JobOutcome.Conflicted, is JobOutcome.Failed -> true
    JobOutcome.Completed, JobOutcome.UpToDate, JobOutcome.NotCheckedOut, JobOutcome.NotConfigured -> false
}
