package io.github.rhizonymph.rostrum.data.session

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.DesktopGitHubToken
import io.github.rhizonymph.rostrum.data.model.PairingResult
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.model.UserRef
import io.github.rhizonymph.rostrum.data.requiresPairing
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.secrets.SecretRead
import io.github.rhizonymph.rostrum.data.secrets.SecretStore
import io.github.rhizonymph.rostrum.data.secrets.SecretWrite
import io.github.rhizonymph.rostrum.data.secrets.describe
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** Whether the app has a GitHub token and a desktop, as far as the phone knows. */
sealed interface SessionState {
    /** Secrets are being read and handed to the backend. */
    data object Restoring : SessionState

    data class Ready(val github: GitHubAuth, val desktop: DesktopLink) : SessionState
}

sealed interface GitHubAuth {
    /** [notice] explains an involuntary sign-out (unreadable secret, revoked token). */
    data class SignedOut(val notice: String? = null) : GitHubAuth

    /** A github.com token is set (GitHub Enterprise is not supported). */
    data object SignedIn : GitHubAuth
}

sealed interface DesktopLink {
    data object NotPaired : DesktopLink

    data class Paired(val status: RemoteStatus.Paired) : DesktopLink
}

val SessionState.isSignedIn: Boolean
    get() = this is SessionState.Ready && github is GitHubAuth.SignedIn

val SessionState.isPaired: Boolean
    get() = this is SessionState.Ready && desktop is DesktopLink.Paired

/**
 * The only owner of the app's secrets. At start-up it reads them from the
 * [SecretStore] and hands them to the backend (which keeps them in memory
 * only); after sign-in or pairing it persists what the backend returned.
 */
class SessionRepository(
    private val backend: RostrumBackend,
    private val secrets: SecretStore,
    private val deviceName: String,
) {
    private val _state = MutableStateFlow<SessionState>(SessionState.Restoring)
    val state: StateFlow<SessionState> = _state.asStateFlow()

    private val mutex = Mutex()
    private var restored = false

    /** Restore once per process; later calls return at once. Safe from any caller. */
    suspend fun restore() {
        mutex.withLock {
            if (restored) return
            val github = restoreGitHub()
            val desktop = restoreDesktop()
            _state.value = SessionState.Ready(github, desktop)
            restored = true
            RostrumLog.i(
                TAG, "session_restored",
                "github" to github::class.simpleName,
                "desktop" to desktop::class.simpleName,
            )
        }
    }

    private suspend fun restoreGitHub(): GitHubAuth {
        return when (val token = secrets.read(SecretKey.GitHubToken)) {
            SecretRead.Absent -> GitHubAuth.SignedOut()
            is SecretRead.Failed -> {
                RostrumLog.w(TAG, "github_token_unreadable", "reason" to token.error.describe())
                secrets.delete(SecretKey.GitHubToken)
                GitHubAuth.SignedOut("Your saved sign-in couldn't be read. Sign in again.")
            }
            is SecretRead.Present -> when (val set = backend.setGitHubToken(token.value)) {
                is Outcome.Ok -> GitHubAuth.SignedIn
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "github_token_rejected", "error" to set.error::class.simpleName)
                    GitHubAuth.SignedOut("Your saved sign-in couldn't be used. Sign in again.")
                }
            }
        }
    }

    private suspend fun restoreDesktop(): DesktopLink {
        val endpoint = secrets.read(SecretKey.DesktopEndpoint)
        val deviceToken = secrets.read(SecretKey.DeviceToken)
        if (endpoint !is SecretRead.Present || deviceToken !is SecretRead.Present) {
            if (endpoint is SecretRead.Present || deviceToken is SecretRead.Present) {
                RostrumLog.w(TAG, "pairing_incomplete", "endpoint" to (endpoint is SecretRead.Present))
                forgetDesktopSecrets()
            }
            return DesktopLink.NotPaired
        }
        return when (val remote = backend.setRemote(endpoint.value, deviceToken.value)) {
            is Outcome.Ok -> (remote.value as? RemoteStatus.Paired)?.let { DesktopLink.Paired(it) } ?: DesktopLink.NotPaired
            is Outcome.Err -> {
                RostrumLog.w(TAG, "pairing_restore_failed", "error" to remote.error::class.simpleName)
                DesktopLink.NotPaired
            }
        }
    }

    /** Verify a pasted github.com token with GitHub, then keep it. */
    suspend fun signInWithToken(token: String): Outcome<UserRef> {
        val cleanToken = token.trim()
        if (cleanToken.isEmpty()) return Outcome.Err(BackendError.InvalidInput("Paste a token first"))
        when (val set = backend.setGitHubToken(cleanToken)) {
            is Outcome.Err -> return set
            is Outcome.Ok -> Unit
        }
        return when (val viewer = backend.viewer()) {
            is Outcome.Err -> {
                backend.setGitHubToken(null)
                RostrumLog.i(TAG, "sign_in_rejected", "error" to viewer.error::class.simpleName)
                viewer
            }
            is Outcome.Ok -> {
                persistGitHub(cleanToken)?.let { return Outcome.Err(it) }
                setGitHub(GitHubAuth.SignedIn)
                RostrumLog.i(TAG, "signed_in", "login" to viewer.value.login)
                viewer
            }
        }
    }

    /** Pair from a `rostrum://pair?…` link and keep the pairing (and a handed-over token). */
    suspend fun pairWithLink(uri: String): Outcome<PairingResult> = adopt(backend.pairWithLink(uri, deviceName))

    /** Pair by address and code, pinned to the fingerprint the user compared. */
    suspend fun pairManual(host: String, port: Int, fingerprint: String, code: String): Outcome<PairingResult> =
        adopt(backend.pairManual(host, port, fingerprint, code, deviceName))

    private suspend fun adopt(outcome: Outcome<PairingResult>): Outcome<PairingResult> {
        val result = when (outcome) {
            is Outcome.Err -> return outcome
            is Outcome.Ok -> outcome.value
        }
        val stored = listOf(
            secrets.write(SecretKey.DesktopEndpoint, result.endpoint),
            secrets.write(SecretKey.DeviceToken, result.deviceToken),
        ).filterIsInstance<SecretWrite.Failed>().firstOrNull()
        if (stored != null) {
            backend.clearRemote()
            return Outcome.Err(BackendError.Storage(stored.error.describe()))
        }
        val remote = backend.remoteStatus()
        val paired = (remote as? Outcome.Ok)?.value as? RemoteStatus.Paired
        val github = result.github
        val current = (_state.value as? SessionState.Ready)?.github
        var notice: String? = null
        if (github != null && current !is GitHubAuth.SignedIn) {
            when (val adopted = adoptDesktopToken(github)) {
                null -> Unit
                is BackendError.InvalidInput -> notice = adopted.reason
                else -> return Outcome.Err(adopted)
            }
        }
        if (notice != null) setGitHub(GitHubAuth.SignedOut(notice))
        _state.update { state ->
            val ready = state as? SessionState.Ready ?: SessionState.Ready(GitHubAuth.SignedOut(), DesktopLink.NotPaired)
            ready.copy(desktop = paired?.let { DesktopLink.Paired(it) } ?: DesktopLink.NotPaired)
        }
        RostrumLog.i(TAG, "paired", "machine" to result.machine.name, "github_handed_over" to (github != null))
        return outcome
    }

    /** Ask the paired desktop for its GitHub token (when this phone's copy stopped working). */
    suspend fun refreshTokenFromDesktop(): Outcome<Unit> = when (val fetched = backend.refreshGitHubTokenFromDesktop()) {
        is Outcome.Err -> fetched
        is Outcome.Ok -> adoptDesktopToken(fetched.value)?.let { Outcome.Err(it) } ?: Outcome.Ok(Unit)
    }

    /**
     * Use a token the desktop handed over. The core talks to github.com only,
     * so a token for another host is refused with an [BackendError.InvalidInput]
     * explaining why, and nothing is stored.
     */
    private suspend fun adoptDesktopToken(github: DesktopGitHubToken): BackendError? {
        if (!github.host.equals(RostrumBackend.GITHUB_COM, ignoreCase = true)) {
            RostrumLog.w(TAG, "desktop_token_other_host", "host" to github.host)
            return BackendError.InvalidInput(
                "Your desktop signs in to ${github.host}; Rostrum on Android supports github.com only. Paste a github.com token instead.",
            )
        }
        when (val set = backend.setGitHubToken(github.token)) {
            is Outcome.Err -> return set.error
            is Outcome.Ok -> Unit
        }
        persistGitHub(github.token)?.let { return it }
        setGitHub(GitHubAuth.SignedIn)
        return null
    }

    /** Forget the GitHub token here and in the backend. */
    suspend fun signOut(notice: String? = null) {
        backend.setGitHubToken(null)
        secrets.delete(SecretKey.GitHubToken)
        setGitHub(GitHubAuth.SignedOut(notice))
        RostrumLog.i(TAG, "signed_out", "involuntary" to (notice != null))
    }

    /**
     * Unpair on the desktop, then forget it here. If the desktop already
     * forgot this phone that counts as success; any other failure keeps the
     * pairing (use [forgetDesktop] to drop it regardless).
     */
    suspend fun unpair(): Outcome<Unit> {
        val result = backend.unpair()
        if (result is Outcome.Err && !result.error.requiresPairing) return result
        forgetDesktop()
        return Outcome.Ok(Unit)
    }

    /** Forget the desktop on this phone only. */
    suspend fun forgetDesktop() {
        backend.clearRemote()
        forgetDesktopSecrets()
        _state.update { state ->
            (state as? SessionState.Ready)?.copy(desktop = DesktopLink.NotPaired) ?: state
        }
        RostrumLog.i(TAG, "desktop_forgotten")
    }

    private suspend fun forgetDesktopSecrets() {
        secrets.delete(SecretKey.DesktopEndpoint)
        secrets.delete(SecretKey.DeviceToken)
    }

    private suspend fun persistGitHub(token: String): BackendError? {
        val failed = secrets.write(SecretKey.GitHubToken, token) as? SecretWrite.Failed ?: return null
        RostrumLog.e(TAG, "github_token_not_saved", "reason" to failed.error.describe())
        return BackendError.Storage(failed.error.describe())
    }

    private fun setGitHub(github: GitHubAuth) {
        _state.update { state ->
            (state as? SessionState.Ready)?.copy(github = github) ?: SessionState.Ready(github, DesktopLink.NotPaired)
        }
    }

    private companion object {
        const val TAG = "RostrumSession"
    }
}
