package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/** What a pairing link says, shown before pairing (`remote/types.rs`). */
data class PairingPreview(
    val machine: String,
    /** Every address the desktop listens on, in the order they are tried. */
    val hosts: List<String>,
    val port: Int,
    /** `4F2A · 91C0 · 7E3B`: compare with the desktop's pairing page. */
    val fingerprintShort: String,
    /** The pairing code as the desktop shows it, `XXXX-XXXX`. */
    val code: String,
)

/** What an unpaired desktop said when asked by address. */
data class DesktopProbe(
    val machine: String,
    val apiVersion: Int,
    /** Whether this build speaks the desktop's protocol version. */
    val compatible: Boolean,
    val host: String,
    val port: Int,
    /** Pass to pairManual once the user compared [fingerprintShort]. */
    val fingerprint: String,
    val fingerprintShort: String,
)

/**
 * A completed pairing. Persist [endpoint] and [deviceToken] (and
 * [github]`.token`) in the secret store; hand them back on the next launch.
 */
data class PairingResult(
    val machine: MachineInfo,
    /** The desktop's addresses, port and fingerprint, serialised. Not a secret. */
    val endpoint: String,
    val deviceId: String,
    /** The bearer credential for this device. A secret. */
    val deviceToken: String,
    /** The desktop's GitHub token, when it handed one over. A secret. */
    val github: DesktopGitHubToken?,
)

/** A GitHub token handed over by the desktop. */
class DesktopGitHubToken(
    val token: String,
    /** Where the desktop got it, e.g. `gh auth token`. */
    val source: String,
    val host: String,
) {
    override fun toString(): String = "DesktopGitHubToken(token=redacted, source=$source, host=$host)"
    override fun equals(other: Any?): Boolean =
        other is DesktopGitHubToken && other.token == token && other.source == source && other.host == host
    override fun hashCode(): Int = (token.hashCode() * 31 + source.hashCode()) * 31 + host.hashCode()
}

/** Whether a desktop is set for this session. */
sealed interface RemoteStatus {
    data object NotPaired : RemoteStatus

    data class Paired(
        val hosts: List<String>,
        val port: Int,
        val fingerprintShort: String,
        /** The address that answered most recently. */
        val currentHost: String,
    ) : RemoteStatus
}

/** The paired desktop. */
data class MachineInfo(
    val name: String,
    val version: String,
    val apiVersion: Int,
    /** Repositories with a local clone on the desktop. */
    val clones: List<CloneInfo>,
    /** Whether a stopped rebase or merge is handed to a tmux session there. */
    val handlerConfigured: Boolean,
    /** The desktop's own stash preference. */
    val autostash: Boolean,
)

data class CloneInfo(val repo: String, val path: String)

/** The desktop clone's view of one pull request's branch. */
sealed interface LocalStatus {
    /** No clone of this repository on the desktop. */
    data object NotConfigured : LocalStatus

    /** A clone exists, but no worktree has the branch checked out. */
    data object NotCheckedOut : LocalStatus

    data class CheckedOut(val branch: LocalBranch) : LocalStatus
}

/** A checked-out branch on the desktop. */
data class LocalBranch(
    val worktree: String,
    val branch: String,
    /** Local commits not on `origin/<branch>`. */
    val ahead: Int,
    /** Remote commits not in the local branch. */
    val behind: Int,
    /** Whether the fetch before counting succeeded. */
    val fetched: Boolean,
    /** Why the local operations would refuse to start. */
    val blocker: String?,
    /** A rebase or merge stopped part-way. */
    val inProgress: InProgress?,
    /** The tmux session a stopped operation was handed to. */
    val handoff: HandoffState?,
)

data class InProgress(
    val kind: InProgressKind,
    /** "Rebase onto main stopped on 3 conflicts". */
    val description: String,
    /** Whether abortLocal can abort it (rebase and merge only). */
    val abortable: Boolean,
)

enum class InProgressKind { Rebase, Am, Merge, CherryPick, Revert, Bisect }

data class HandoffState(
    val session: String,
    /** Whether the tmux session still exists. */
    val running: Boolean,
    /** `tmux attach -t =<session>`. */
    val attachCommand: String,
)

/** One local git operation on the desktop. Nothing is ever pushed. */
enum class LocalOp {
    /** Rebase local commits onto `origin/<branch>`. */
    PullRebase,

    /** Merge `origin/<branch>` into the local branch. */
    MergeRemote,

    /** Merge `origin/<base>` into the branch. */
    MergeBase,

    /** Rebase the branch onto `origin/<base>`. */
    RebaseBase,
}

/** The operations "sync all" can run across every checked-out branch. */
enum class SyncAllOp { Pull, MergeBase, RebaseBase }

/** How a local job ended. */
sealed interface JobOutcome {
    data object NotConfigured : JobOutcome
    data object NotCheckedOut : JobOutcome
    data object UpToDate : JobOutcome
    data object Completed : JobOutcome

    /** Git would not start (dirty worktree, operation in progress, …). */
    data class Refused(val reason: String) : JobOutcome

    /** Stopped on conflicts and aborted; the worktree is as it was. */
    data class Conflicted(val reason: String) : JobOutcome

    /** Stopped on conflicts and handed to a tmux session on the desktop. */
    data class HandedOff(val session: String, val attachCommand: String) : JobOutcome

    data class Failed(val reason: String) : JobOutcome
}

/** A job's outcome with its render-ready description. */
data class JobResult(
    val outcome: JobOutcome,
    /** One line for a snackbar. */
    val detail: String,
    /** `refused`, `conflict`, `handed off`, `failed`; `null` for success. */
    val chip: Chip?,
)

/** A "sync all" run on the desktop, running or finished. */
data class SyncRun(
    val id: Long,
    val op: LocalOp,
    val startedAt: Instant,
    val finishedAt: Instant?,
    val entries: List<SyncEntry>,
    val summary: SyncSummary,
    /** `3/12…` while running; `9 updated, 2 handed off` when done. */
    val progressText: String,
) {
    val running: Boolean get() = finishedAt == null
}

data class SyncEntry(
    val repo: String,
    val number: Int,
    val headRef: String,
    val state: SyncEntryState,
)

sealed interface SyncEntryState {
    data object Pending : SyncEntryState
    data object Running : SyncEntryState
    data class Done(val result: JobResult) : SyncEntryState
}

data class SyncSummary(
    val total: Int,
    val done: Int,
    val updated: Int,
    val upToDate: Int,
    val handedOff: Int,
    val conflicts: Int,
    val refused: Int,
    val failed: Int,
    val skipped: Int,
)

/** A tmux session on the desktop holding a stopped rebase or merge. */
data class HandoffSession(
    val session: String,
    val repo: String?,
    val number: Int?,
    val headRef: String?,
    val worktree: String?,
    val startedAt: Instant?,
    val attachCommand: String,
) {
    val pr: PrRef? get() = if (repo != null && number != null) PrRef(repo, number) else null
}
