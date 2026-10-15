package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.CloneInfo
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.HandoffState
import io.github.rhizonymph.rostrum.data.model.InProgress
import io.github.rhizonymph.rostrum.data.model.InProgressKind
import io.github.rhizonymph.rostrum.data.model.JobOutcome
import io.github.rhizonymph.rostrum.data.model.JobResult
import io.github.rhizonymph.rostrum.data.model.LocalBranch
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.SyncEntry
import io.github.rhizonymph.rostrum.data.model.SyncEntryState
import io.github.rhizonymph.rostrum.data.model.SyncRun
import io.github.rhizonymph.rostrum.data.model.SyncSummary
import java.time.Duration
import java.time.Instant

/** The paired desktop of the mockups: nymph-desk. */
internal object SampleDesktop {
    const val MACHINE = "nymph-desk"
    const val FINGERPRINT = "sha256:4F2A91C07E3BD5186A0C44E1B2F9D7C35E6A8B90F1D2C3B4A5968778695A4B3C"
    const val FINGERPRINT_SHORT = "4F2A · 91C0 · 7E3B"
    const val PAIRING_CODE = "WDJB-MJHT"
    val hosts = listOf("192.168.1.24", "nymph-desk.local")
    const val PORT = 8485
    const val HANDOFF_SESSION = "rostrum-RhizoNymph-rostrum-10"

    fun machine(): MachineInfo = MachineInfo(
        name = MACHINE,
        version = "0.1.0",
        apiVersion = 1,
        clones = listOf(
            CloneInfo(SamplePulls.ROSTRUM, "~/Code/devtools/rostrum"),
            CloneInfo(SamplePulls.ZED, "~/Code/zed"),
        ),
        handlerConfigured = true,
        autostash = false,
    )

    fun attach(session: String) = "tmux attach -t =$session"

    /** nymph-desk's own settings, which "copy settings" brings to the phone. */
    val configRepos = listOf(SamplePulls.ROSTRUM, SamplePulls.ZED, SamplePulls.TOKIO, "serde-rs/serde")
    const val CONFIG_PRS_PER_REPO = 25
    const val CONFIG_ISSUES_PER_REPO = 25
    val configPreferences = FeedPreferences(hideDrafts = true, hideEmptyRepos = true, authors = emptyList(), includeInvolved = false)
    const val CONFIG_AUTOSTASH = true

    /** Every pull request's local state on the desktop, keyed by pull request. */
    fun localStatuses(): MutableMap<PrRef, LocalStatus> = mutableMapOf(
        PrRef(SamplePulls.ROSTRUM, 10) to LocalStatus.CheckedOut(
            LocalBranch(
                worktree = "~/Code/devtools/rostrum/feat-diff-overview",
                branch = "feat/diff-overview",
                ahead = 0,
                behind = 0,
                fetched = true,
                blocker = null,
                inProgress = InProgress(InProgressKind.Rebase, "Rebase onto main stopped on 3 conflicts", abortable = true),
                handoff = HandoffState(HANDOFF_SESSION, running = true, attachCommand = attach(HANDOFF_SESSION)),
            ),
        ),
        PrRef(SamplePulls.ROSTRUM, 9) to LocalStatus.CheckedOut(
            LocalBranch(
                worktree = "~/Code/devtools/rostrum/feat-author-filter",
                branch = "feat/author-filter",
                ahead = 2,
                behind = 0,
                fetched = true,
                blocker = "worktree has uncommitted changes",
                inProgress = null,
                handoff = null,
            ),
        ),
        PrRef(SamplePulls.ROSTRUM, 11) to LocalStatus.NotCheckedOut,
        PrRef(SamplePulls.ZED, 38112) to clean("~/Code/zed/tj-git-status-allocs", "tj/git-status-allocs", behind = 1),
        PrRef(SamplePulls.ZED, 38090) to clean("~/Code/zed/mkowal-diff-soft-wrap", "mkowal/diff-soft-wrap", behind = 0),
        PrRef(SamplePulls.ZED, 38120) to clean("~/Code/zed/jm-split-flicker", "jm/split-flicker", behind = 2),
        PrRef(SamplePulls.ZED, 38101) to clean("~/Code/zed/wren-vim-gv", "wren/vim-gv", behind = 0),
        PrRef(SamplePulls.ZED, 38150) to LocalStatus.NotCheckedOut,
        PrRef(SamplePulls.ZED, 38077) to LocalStatus.NotCheckedOut,
    )

    private fun clean(worktree: String, branch: String, behind: Int) = LocalStatus.CheckedOut(
        LocalBranch(worktree, branch, ahead = 0, behind = behind, fetched = true, blocker = null, inProgress = null, handoff = null),
    )

    fun result(outcome: JobOutcome, branch: String): JobResult = when (outcome) {
        JobOutcome.NotConfigured -> JobResult(outcome, "No clone of this repository on the desktop", Chip("no clone", ColorRole.Neutral))
        JobOutcome.NotCheckedOut -> JobResult(outcome, "$branch isn't checked out in any worktree", Chip("not checked out", ColorRole.Neutral))
        JobOutcome.UpToDate -> JobResult(outcome, "$branch is already up to date", null)
        JobOutcome.Completed -> JobResult(outcome, "Updated $branch", null)
        is JobOutcome.Refused -> JobResult(outcome, outcome.reason, Chip("refused", ColorRole.Warning))
        is JobOutcome.Conflicted -> JobResult(outcome, outcome.reason, Chip("conflict", ColorRole.Danger))
        is JobOutcome.HandedOff -> JobResult(outcome, "Conflicts handed to tmux ${outcome.session}", Chip("handed off", ColorRole.Accent))
        is JobOutcome.Failed -> JobResult(outcome, outcome.reason, Chip("failed", ColorRole.Danger))
    }

    /** The last "sync all": a rebase of everything, three minutes ago. */
    fun lastRun(now: Instant): SyncRun {
        val finished = now.minus(Duration.ofMinutes(3))
        fun done(repo: String, number: Int, headRef: String, outcome: JobOutcome) =
            SyncEntry(repo, number, headRef, SyncEntryState.Done(result(outcome, headRef)))
        val entries = listOf(
            done(SamplePulls.ROSTRUM, 10, "feat/diff-overview",
                JobOutcome.HandedOff(HANDOFF_SESSION, attach(HANDOFF_SESSION))),
            done(SamplePulls.ROSTRUM, 9, "feat/author-filter", JobOutcome.Refused("worktree has uncommitted changes")),
            done(SamplePulls.ZED, 38112, "tj/git-status-allocs", JobOutcome.Completed),
            done(SamplePulls.ZED, 38090, "mkowal/diff-soft-wrap", JobOutcome.Completed),
            done(SamplePulls.ZED, 38120, "jm/split-flicker", JobOutcome.Completed),
            done(SamplePulls.ZED, 38101, "wren/vim-gv", JobOutcome.Completed),
            done(SamplePulls.ZED, 38064, "kb/lsp-restart", JobOutcome.Completed),
            done(SamplePulls.ZED, 38031, "ol/outline-cache", JobOutcome.Completed),
        )
        return SyncRun(
            id = 1,
            op = LocalOp.RebaseBase,
            startedAt = finished.minus(Duration.ofSeconds(41)),
            finishedAt = finished,
            entries = entries,
            summary = summarize(entries),
            progressText = "6 updated, 1 handed off, 1 refused",
        )
    }

    fun summarize(entries: List<SyncEntry>): SyncSummary {
        val outcomes = entries.mapNotNull { (it.state as? SyncEntryState.Done)?.result?.outcome }
        return SyncSummary(
            total = entries.size,
            done = outcomes.size,
            updated = outcomes.count { it == JobOutcome.Completed },
            upToDate = outcomes.count { it == JobOutcome.UpToDate },
            handedOff = outcomes.count { it is JobOutcome.HandedOff },
            conflicts = outcomes.count { it is JobOutcome.Conflicted },
            refused = outcomes.count { it is JobOutcome.Refused },
            failed = outcomes.count { it is JobOutcome.Failed },
            skipped = outcomes.count { it == JobOutcome.NotCheckedOut || it == JobOutcome.NotConfigured },
        )
    }

    fun progressText(summary: SyncSummary, finished: Boolean): String {
        if (!finished) return "${summary.done}/${summary.total}…"
        val parts = buildList {
            if (summary.updated > 0) add("${summary.updated} updated")
            if (summary.upToDate > 0) add("${summary.upToDate} up to date")
            if (summary.handedOff > 0) add("${summary.handedOff} handed off")
            if (summary.conflicts > 0) add("${summary.conflicts} conflicted")
            if (summary.refused > 0) add("${summary.refused} refused")
            if (summary.failed > 0) add("${summary.failed} failed")
        }
        return parts.joinToString(", ").ifEmpty { "Nothing to do" }
    }
}
