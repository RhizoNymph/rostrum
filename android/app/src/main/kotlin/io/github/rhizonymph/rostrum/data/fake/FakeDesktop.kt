package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.model.DesktopGitHubToken
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.HandoffSession
import io.github.rhizonymph.rostrum.data.model.HandoffState
import io.github.rhizonymph.rostrum.data.model.InProgress
import io.github.rhizonymph.rostrum.data.model.InProgressKind
import io.github.rhizonymph.rostrum.data.model.JobOutcome
import io.github.rhizonymph.rostrum.data.model.JobResult
import io.github.rhizonymph.rostrum.data.model.LocalOp
import io.github.rhizonymph.rostrum.data.model.LocalStatus
import io.github.rhizonymph.rostrum.data.model.MachineInfo
import io.github.rhizonymph.rostrum.data.model.MergeStatus
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.PairingResult
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.PullState
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.SyncAllOp
import io.github.rhizonymph.rostrum.data.model.SyncEntry
import io.github.rhizonymph.rostrum.data.model.SyncEntryState
import io.github.rhizonymph.rostrum.data.model.SyncRun
import java.net.URI
import java.net.URLDecoder
import java.time.Clock
import java.time.Duration
import java.time.Instant

/**
 * The fake's paired desktop (nymph-desk): pairing, the session's remote,
 * local worktree state and jobs, "sync all" and handoff sessions. Owned by
 * [FakeRostrumBackend], which serialises access and injects failures; the
 * callbacks reach back into its GitHub session and feed.
 */
internal class FakeDesktop(
    private val clock: Clock,
    private val started: Instant,
    paired: Boolean,
    private val gitHubHost: String,
    private val pulls: MutableMap<PrRef, FakePull>,
    private val onFeedChanged: () -> Unit,
    /** The desktop handed over a GitHub token: use it if none is set. */
    private val adoptTokenIfSignedOut: (String) -> Unit,
    /** The desktop's token replaces the current one. */
    private val replaceToken: (String) -> Unit,
) {
    private var remote: RemoteStatus = if (paired) pairedStatus(SampleDesktop.hosts) else RemoteStatus.NotPaired
    private val local = SampleDesktop.localStatuses()
    private var syncRun: SyncRun? = SampleDesktop.lastRun(started)
    private var syncAutostash = false
    private var nextId = 1L

    private fun <T> withRemote(block: () -> Outcome<T>): Outcome<T> =
        if (remote is RemoteStatus.NotPaired) Outcome.Err(BackendError.NotPaired) else block()

    fun parsePairingLink(uri: String): Outcome<PairingPreview> {
        val parsed = parseUri(uri) ?: return Outcome.Err(BackendError.InvalidInput("That isn't a Rostrum pairing link"))
        if (parsed.scheme != "rostrum" || parsed.host != "pair") {
            return Outcome.Err(BackendError.InvalidInput("That isn't a Rostrum pairing link"))
        }
        val params = parsed.rawQuery.orEmpty().split('&').filter { '=' in it }.associate {
            val (k, v) = it.split('=', limit = 2)
            k to URLDecoder.decode(v, Charsets.UTF_8)
        }
        // The core's keys (v, m, h, p, c, fp), or the fake's longer spellings.
        fun param(short: String, long: String) = params[short] ?: params[long]
        val code = param("c", "code")?.takeIf { it.isNotBlank() }
            ?: return Outcome.Err(BackendError.InvalidInput("The link has no pairing code"))
        return Outcome.Ok(
            PairingPreview(
                machine = param("m", "name") ?: SampleDesktop.MACHINE,
                hosts = param("h", "hosts")?.split(',')?.filter { it.isNotBlank() } ?: SampleDesktop.hosts,
                port = param("p", "port")?.toIntOrNull() ?: SampleDesktop.PORT,
                fingerprintShort = params["fp"]?.let(FakeRostrumBackend::shortFingerprint) ?: SampleDesktop.FINGERPRINT_SHORT,
                code = code,
            ),
        )
    }

    private fun parseUri(uri: String): URI? = try {
        URI(uri.trim())
    } catch (e: java.net.URISyntaxException) {
        null
    }

    private fun pairedStatus(hosts: List<String>) = RemoteStatus.Paired(
        hosts = hosts,
        port = SampleDesktop.PORT,
        fingerprintShort = SampleDesktop.FINGERPRINT_SHORT,
        currentHost = hosts.first(),
    )

    private fun completePairing(machine: String, hosts: List<String>): PairingResult {
        remote = pairedStatus(hosts)
        val github = DesktopGitHubToken(DESKTOP_TOKEN, "gh auth token", gitHubHost)
        adoptTokenIfSignedOut(github.token)
        return PairingResult(
            machine = SampleDesktop.machine().copy(name = machine),
            endpoint = """{"hosts":[${hosts.joinToString(",") { "\"$it\"" }}],"port":${SampleDesktop.PORT},"fingerprint":"${SampleDesktop.FINGERPRINT}"}""",
            deviceId = "device-${nextId++}",
            deviceToken = "rdt_sample_${nextId++}",
            github = github,
        )
    }

    fun pairWithLink(uri: String, deviceName: String): Outcome<PairingResult> = run {
        when (val preview = parsePairingLink(uri)) {
            is Outcome.Err -> preview
            is Outcome.Ok -> Outcome.Ok(completePairing(preview.value.machine, preview.value.hosts))
        }
    }

    fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> = run {
        val trimmed = host.trim()
        when {
            trimmed.isEmpty() -> Outcome.Err(BackendError.InvalidInput("Enter the desktop's address"))
            port !in 1..65535 -> Outcome.Err(BackendError.InvalidInput("Ports run from 1 to 65535"))
            "unreachable" in trimmed -> Outcome.Err(BackendError.DesktopUnreachable("connection refused by $trimmed:$port"))
            else -> Outcome.Ok(
                DesktopProbe(
                    machine = SampleDesktop.MACHINE,
                    apiVersion = 1,
                    compatible = true,
                    host = trimmed,
                    port = port,
                    fingerprint = SampleDesktop.FINGERPRINT,
                    fingerprintShort = SampleDesktop.FINGERPRINT_SHORT,
                ),
            )
        }
    }

    fun pairManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<PairingResult> = run {
        val normalized = code.filter { it.isLetterOrDigit() }.uppercase()
        when {
            normalized.length != 8 -> Outcome.Err(BackendError.RemoteApi(RemoteErrorCode.PairingCodeInvalid, "the code has 8 characters"))
            normalized == "00000000" -> Outcome.Err(BackendError.RemoteApi(RemoteErrorCode.PairingCodeExpired, "the code expired"))
            fingerprint != SampleDesktop.FINGERPRINT -> Outcome.Err(BackendError.CertificateMismatch(host))
            else -> Outcome.Ok(completePairing(SampleDesktop.MACHINE, listOf(host.trim())))
        }
    }

    fun setRemote(endpoint: String, deviceToken: String): Outcome<RemoteStatus> = run {
        if (endpoint.isBlank() || deviceToken.isBlank()) {
            Outcome.Err(BackendError.InvalidInput("The saved pairing is incomplete"))
        } else {
            val hosts = Regex("\"hosts\":\\[([^]]*)]").find(endpoint)?.groupValues?.get(1)
                ?.split(',')?.map { it.trim().trim('"') }?.filter { it.isNotEmpty() }
                ?.takeIf { it.isNotEmpty() } ?: SampleDesktop.hosts
            remote = pairedStatus(hosts)
            Outcome.Ok(remote)
        }
    }

    fun clearRemote(): Outcome<Unit> = run {
        remote = RemoteStatus.NotPaired
        Outcome.Ok(Unit)
    }

    fun remoteStatus(): Outcome<RemoteStatus> = run { Outcome.Ok(remote) }

    fun machineInfo(autostash: Boolean): Outcome<MachineInfo> =
        run { withRemote { Outcome.Ok(SampleDesktop.machine().copy(autostash = autostash)) } }

    private fun localOf(pr: PrRef): LocalStatus = local[pr]
        ?: if (SampleDesktop.machine().clones.any { it.repo == pr.repo }) LocalStatus.NotCheckedOut else LocalStatus.NotConfigured

    fun localStatus(pr: PrRef): Outcome<LocalStatus> =
        run { withRemote { Outcome.Ok(localOf(pr)) } }

    private fun runJob(pr: PrRef, op: LocalOp, autostash: Boolean): JobResult {
        val status = localOf(pr)
        val branch = (status as? LocalStatus.CheckedOut)?.branch
        val name = branch?.branch ?: pulls[pr]?.headRef.orEmpty()
        val outcome: JobOutcome = when {
            status == LocalStatus.NotConfigured -> JobOutcome.NotConfigured
            branch == null -> JobOutcome.NotCheckedOut
            branch.inProgress != null -> JobOutcome.Refused(branch.inProgress.description.lowercase())
            branch.blocker != null && !autostash -> JobOutcome.Refused(branch.blocker)
            op == LocalOp.PullRebase || op == LocalOp.MergeRemote ->
                if (branch.behind == 0) JobOutcome.UpToDate else JobOutcome.Completed
            (pulls[pr]?.behind ?: 0) == 0 -> JobOutcome.UpToDate
            pulls[pr]?.mergeStatus == MergeStatus.Conflicts -> {
                val session = "rostrum-${pr.repo.replace('/', '-')}-${pr.number}"
                JobOutcome.HandedOff(session, SampleDesktop.attach(session))
            }
            else -> JobOutcome.Completed
        }
        if (branch != null) {
            local[pr] = LocalStatus.CheckedOut(
                when (outcome) {
                    JobOutcome.Completed -> branch.copy(
                        behind = 0,
                        ahead = if (op == LocalOp.MergeBase || op == LocalOp.RebaseBase) branch.ahead + (pulls[pr]?.behind ?: 0) else branch.ahead,
                        blocker = if (autostash) null else branch.blocker,
                    )
                    is JobOutcome.HandedOff -> branch.copy(
                        inProgress = InProgress(
                            InProgressKind.Rebase,
                            "Rebase onto ${pulls[pr]?.baseRef ?: "main"} stopped on conflicts",
                            abortable = true,
                        ),
                        handoff = HandoffState(outcome.session, true, outcome.attachCommand),
                    )
                    else -> branch
                },
            )
        }
        return SampleDesktop.result(outcome, name)
    }

    fun runLocalJob(pr: PrRef, op: LocalOp, autostash: Boolean): Outcome<JobResult> =
        run { withRemote { Outcome.Ok(runJob(pr, op, autostash)).also { onFeedChanged() } } }

    fun abortLocal(pr: PrRef): Outcome<Unit> = run {
        withRemote {
            val branch = (local[pr] as? LocalStatus.CheckedOut)?.branch
            if (branch?.inProgress == null || !branch.inProgress.abortable) {
                Outcome.Err(BackendError.RemoteApi(RemoteErrorCode.BadRequest, "Nothing to abort in this worktree"))
            } else {
                local[pr] = LocalStatus.CheckedOut(branch.copy(inProgress = null, handoff = null))
                onFeedChanged()
                Outcome.Ok(Unit)
            }
        }
    }

    fun startSyncAll(op: SyncAllOp, autostash: Boolean): Outcome<SyncRun> = run {
        withRemote {
            if (syncRun?.running == true) {
                return@withRemote Outcome.Err(BackendError.RemoteApi(RemoteErrorCode.Busy, "a sync is already running"))
            }
            val entries = local.entries
                .filter { (pr, status) -> status is LocalStatus.CheckedOut && pulls[pr]?.state == PullState.Open }
                .map { (pr, status) -> SyncEntry(pr.repo, pr.number, (status as LocalStatus.CheckedOut).branch.branch, SyncEntryState.Pending) }
            val summary = SampleDesktop.summarize(entries)
            val run = SyncRun(
                id = (syncRun?.id ?: 0) + 1,
                op = when (op) {
                    SyncAllOp.Pull -> LocalOp.PullRebase
                    SyncAllOp.MergeBase -> LocalOp.MergeBase
                    SyncAllOp.RebaseBase -> LocalOp.RebaseBase
                },
                startedAt = clock.instant(),
                finishedAt = null,
                entries = entries,
                summary = summary,
                progressText = SampleDesktop.progressText(summary, finished = false),
            )
            syncRun = run
            syncAutostash = autostash
            Outcome.Ok(run)
        }
    }

    /** Each poll finishes one more entry, so progress is visible and deterministic. */
    fun syncAllStatus(): Outcome<SyncRun?> = run {
        withRemote {
            val run = syncRun
            if (run != null && run.running) {
                val index = run.entries.indexOfFirst { it.state !is SyncEntryState.Done }
                val entries = run.entries.toMutableList()
                if (index >= 0) {
                    val entry = entries[index]
                    entries[index] = entry.copy(state = SyncEntryState.Done(runJob(PrRef(entry.repo, entry.number), run.op, syncAutostash)))
                    if (index + 1 < entries.size) entries[index + 1] = entries[index + 1].copy(state = SyncEntryState.Running)
                }
                val summary = SampleDesktop.summarize(entries)
                val finished = summary.done == summary.total
                syncRun = run.copy(
                    entries = entries,
                    summary = summary,
                    finishedAt = if (finished) clock.instant() else null,
                    progressText = SampleDesktop.progressText(summary, finished),
                )
                if (finished) onFeedChanged()
            }
            Outcome.Ok(syncRun)
        }
    }

    fun handoffs(): Outcome<List<HandoffSession>> = run {
        withRemote {
            Outcome.Ok(
                local.mapNotNull { (pr, status) ->
                    val branch = (status as? LocalStatus.CheckedOut)?.branch ?: return@mapNotNull null
                    val handoff = branch.handoff?.takeIf { it.running } ?: return@mapNotNull null
                    HandoffSession(
                        session = handoff.session,
                        repo = pr.repo,
                        number = pr.number,
                        headRef = branch.branch,
                        worktree = branch.worktree,
                        startedAt = started.minus(Duration.ofMinutes(3)),
                        attachCommand = handoff.attachCommand,
                    )
                },
            )
        }
    }

    fun refreshGitHubTokenFromDesktop(): Outcome<DesktopGitHubToken> =
        run {
            withRemote {
                val github = DesktopGitHubToken(DESKTOP_TOKEN, "gh auth token", gitHubHost)
                replaceToken(github.token)
                Outcome.Ok(github)
            }
        }

    fun unpair(): Outcome<Unit> = run {
        withRemote {
            remote = RemoteStatus.NotPaired
            Outcome.Ok(Unit)
        }
    }

    private companion object {
        const val DESKTOP_TOKEN = "gho_desktopHandedOverSampleToken01"
    }
}
