package io.github.rhizonymph.rostrum.data.profiles

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfilePairing
import io.github.rhizonymph.rostrum.data.secrets.SecretStore
import io.github.rhizonymph.rostrum.data.secrets.SecretWrite
import io.github.rhizonymph.rostrum.data.secrets.describe
import io.github.rhizonymph.rostrum.data.secrets.forProfile
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isSignedIn
import io.github.rhizonymph.rostrum.data.valueOrNull
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.concurrent.ConcurrentHashMap

/**
 * Owns the profile registry, each profile's backend and session, and which
 * profile is active. Everything that changes the set of profiles goes
 * through here, so [state] is always what the registry holds.
 *
 * Each profile's secrets live under its id in the [SecretStore]; its session
 * sees only those. [scope] outlives every screen: it runs the watchers that
 * keep each profile's GitHub login in the registry.
 */
class ProfileManager(
    private val registry: ProfileRegistryApi,
    private val secrets: SecretStore,
    private val deviceName: String,
    private val scope: CoroutineScope,
    private val legacy: LegacyCleanup = LegacyCleanup.None,
) {
    private val _state = MutableStateFlow<ProfilesState>(ProfilesState.Starting)
    val state: StateFlow<ProfilesState> = _state.asStateFlow()

    private val _signedIn = MutableStateFlow<Set<ProfileId>>(emptySet())

    /** The profiles whose session holds a working GitHub token. */
    val signedInProfiles: StateFlow<Set<ProfileId>> = _signedIn.asStateFlow()

    private class Entry(val handle: ProfileHandle, val watcher: Job)

    private val _upgradedFromSingleProfile = MutableStateFlow(false)

    /**
     * This start wiped a single-profile build's sign-in and pairing, so the
     * first-run screen explains why it is back and how to pair again.
     */
    val upgradedFromSingleProfile: StateFlow<Boolean> = _upgradedFromSingleProfile.asStateFlow()

    private val entries = ConcurrentHashMap<ProfileId, Entry>()
    private val startLock = Mutex()

    /** Serialises changes to the registry and the publishing of [state]. */
    private val ops = Mutex()

    /**
     * Clear the legacy state, open the registry, pick the active profile
     * (the most recently used when none is set) and restore every profile's
     * secrets into its core, the active one first. Returns once all are
     * restored; later calls return at once. After a failure it may be called
     * again.
     */
    suspend fun start() {
        startLock.withLock {
            if (_state.value is ProfilesState.Ready) return
            _state.value = ProfilesState.Starting
            if (legacy.wipe()) _upgradedFromSingleProfile.value = true
            val ready = ops.withLock { openRegistry() }
            if (ready == null) return
            val order = listOfNotNull(ready.active) + ready.profiles.map { it.id }.filter { it != ready.active }
            order.forEach { handle(it).session.restore() }
            RostrumLog.i(TAG, "profiles_restored", "count" to order.size, "active" to ready.active)
        }
    }

    private suspend fun openRegistry(): ProfilesState.Ready? {
        val profiles = when (val listed = registry.profiles()) {
            is Outcome.Err -> return unavailable(listed.error)
            is Outcome.Ok -> listed.value
        }
        val active = when (val current = registry.activeProfile()) {
            is Outcome.Err -> return unavailable(current.error)
            is Outcome.Ok -> current.value?.takeIf { id -> profiles.any { it.id == id } }
        }
        val fallback = profiles.firstOrNull()
        if (active == null && fallback != null) {
            when (val set = registry.setActiveProfile(fallback.id)) {
                is Outcome.Err -> return unavailable(set.error)
                is Outcome.Ok -> Unit
            }
        }
        return refreshLocked() as? ProfilesState.Ready
    }

    private fun unavailable(error: BackendError): ProfilesState.Ready? {
        RostrumLog.e(TAG, "profiles_unavailable", "error" to error::class.simpleName)
        _state.value = ProfilesState.Unavailable(error)
        return null
    }

    /** Re-read the registry into [state]. Call with [ops] held. */
    private suspend fun refreshLocked(): ProfilesState {
        val profiles = when (val listed = registry.profiles()) {
            is Outcome.Err -> return _state.value.also {
                RostrumLog.w(TAG, "profiles_list_failed", "error" to listed.error::class.simpleName)
            }
            is Outcome.Ok -> listed.value
        }
        val active = registry.activeProfile().valueOrNull()?.takeIf { id -> profiles.any { it.id == id } }
        return ProfilesState.Ready(profiles, active).also { _state.value = it }
    }

    /**
     * The backend and session of [id], made on first use. Only ask for ids
     * in [state]; a removed profile's handle is dropped.
     */
    fun handle(id: ProfileId): ProfileHandle = entries.computeIfAbsent(id, ::newEntry).handle

    private fun newEntry(id: ProfileId): Entry {
        val backend = registry.backend(id)
        val handle = ProfileHandle(id, backend, SessionRepository(backend, secrets.forProfile(id)))
        return Entry(handle, scope.launch { watchSignIn(handle) })
    }

    /** Keep [signedInProfiles] and the registry's GitHub login in step with the session. */
    private suspend fun watchSignIn(handle: ProfileHandle) {
        handle.session.state
            .filterIsInstance<SessionState.Ready>()
            .map { it.github is GitHubAuth.SignedIn }
            .distinctUntilChanged()
            .collect { signedIn ->
                _signedIn.update { if (signedIn) it + handle.id else it - handle.id }
                val login = if (signedIn) handle.backend.viewer().valueOrNull()?.login ?: return@collect else null
                recordLogin(handle.id, login)
            }
    }

    private suspend fun recordLogin(id: ProfileId, login: String?) {
        ops.withLock {
            val profile = (_state.value as? ProfilesState.Ready)?.profile(id) ?: return
            if (profile.githubLogin == login) return
            when (val set = registry.setProfileLogin(id, login)) {
                is Outcome.Err -> RostrumLog.w(TAG, "profile_login_not_saved", "profile" to id, "error" to set.error::class.simpleName)
                is Outcome.Ok -> refreshLocked()
            }
        }
    }

    /** Make [id] the active profile; the app then shows it from its feed. */
    suspend fun switchTo(id: ProfileId): Outcome<Profile> {
        val ready = _state.value as? ProfilesState.Ready ?: return Outcome.Err(NOT_LOADED)
        if (ready.profile(id) == null) return Outcome.Err(BackendError.ProfileNotFound(id.value))
        handle(id).session.restore()
        return ops.withLock {
            when (val set = registry.setActiveProfile(id)) {
                is Outcome.Err -> set
                is Outcome.Ok -> {
                    refreshLocked()
                    RostrumLog.i(TAG, "profile_switched", "profile" to id)
                    set
                }
            }
        }
    }

    /**
     * A new profile for a pasted github.com [token], checked with GitHub
     * first. Named [label], or after the token's login when that is blank.
     * Not made active. Nothing is kept when GitHub rejects the token.
     */
    suspend fun createTokenProfile(token: String, label: String?): Outcome<Profile> {
        val cleanToken = token.trim()
        if (cleanToken.isEmpty()) return Outcome.Err(BackendError.InvalidInput("Paste a token first"))
        val requested = label?.trim()?.takeIf { it.isNotEmpty() }
        val created = when (val made = ops.withLock { registry.createTokenProfile(requested ?: DEFAULT_TOKEN_LABEL) }) {
            is Outcome.Err -> return made
            is Outcome.Ok -> made.value
        }
        val session = handle(created.id).session
        session.restore()
        val login = when (val signedIn = session.signInWithToken(cleanToken)) {
            is Outcome.Err -> {
                discard(created.id)
                return signedIn
            }
            is Outcome.Ok -> signedIn.value.login
        }
        ops.withLock {
            if (requested == null) registry.renameProfile(created.id, login).logErr("profile_rename_failed")
            registry.setProfileLogin(created.id, login).logErr("profile_login_not_saved")
            refreshLocked()
        }
        RostrumLog.i(TAG, "token_profile_created", "profile" to created.id, "login" to login)
        return Outcome.Ok(profileOrElse(created))
    }

    /** Pair from a `rostrum://pair?…` link into a new profile, or the one already paired with that desktop. */
    suspend fun pairWithLink(uri: String): Outcome<PairedProfile> =
        adopt(registry.pairDesktopWithLink(uri, deviceName))

    /** Pair by address and code, pinned to the fingerprint the user compared. */
    suspend fun pairManual(host: String, port: Int, fingerprint: String, code: String): Outcome<PairedProfile> =
        adopt(registry.pairDesktopManual(host, port, fingerprint, code, deviceName))

    private suspend fun adopt(outcome: Outcome<ProfilePairing>): Outcome<PairedProfile> {
        val paired = when (outcome) {
            is Outcome.Err -> return outcome
            is Outcome.Ok -> outcome.value
        }
        val id = paired.profile.id
        val session = handle(id).session
        session.restore()
        when (val kept = session.adoptPairing(paired.pairing)) {
            is Outcome.Err -> {
                if (paired.created) discard(id)
                return kept
            }
            is Outcome.Ok -> Unit
        }
        ops.withLock { refreshLocked() }
        RostrumLog.i(TAG, "desktop_paired", "profile" to id, "created" to paired.created, "machine" to paired.pairing.machine.name)
        return Outcome.Ok(PairedProfile(profileOrElse(paired.profile), paired.created, paired.pairing.machine.name))
    }

    /** Drop a profile this manager just made and could not finish setting up. */
    private suspend fun discard(id: ProfileId) {
        ops.withLock {
            registry.removeProfile(id).logErr("profile_discard_failed")
            forget(id)
            refreshLocked()
        }
    }

    private suspend fun forget(id: ProfileId) {
        entries.remove(id)?.watcher?.cancel()
        _signedIn.update { it - id }
        val deleted = secrets.deleteProfile(id)
        if (deleted is SecretWrite.Failed) {
            RostrumLog.w(TAG, "profile_secrets_not_deleted", "profile" to id, "reason" to deleted.error.describe())
        }
    }

    suspend fun rename(id: ProfileId, label: String): Outcome<Profile> {
        val clean = label.trim()
        if (clean.isEmpty()) return Outcome.Err(BackendError.InvalidInput("Give the profile a name"))
        return ops.withLock {
            when (val renamed = registry.renameProfile(id, clean)) {
                is Outcome.Err -> renamed
                is Outcome.Ok -> {
                    refreshLocked()
                    renamed
                }
            }
        }
    }

    /**
     * Unpair (best-effort), delete the profile's data and its secrets. When
     * it was active, the most recently used of the rest becomes active; when
     * none remain, none is.
     */
    suspend fun remove(id: ProfileId): Outcome<ProfileRemoval> {
        val removed = (_state.value as? ProfilesState.Ready)?.profile(id)
            ?: return Outcome.Err(BackendError.ProfileNotFound(id.value))
        // The registry unpairs only through an open core with its remote set: restore it first.
        handle(id).session.restore()
        val removal = ops.withLock {
            val wasActive = (_state.value as? ProfilesState.Ready)?.active == id
            when (val gone = registry.removeProfile(id)) {
                is Outcome.Err -> return gone
                is Outcome.Ok -> forget(id)
            }
            val next = if (wasActive) registry.profiles().valueOrNull()?.firstOrNull() else null
            if (next != null) {
                handle(next.id).session.restore()
                registry.setActiveProfile(next.id).logErr("profile_switch_failed")
            }
            refreshLocked()
            when {
                !wasActive -> ProfileRemoval.Inactive(removed)
                next != null -> ProfileRemoval.ActiveReplaced(removed, next)
                else -> ProfileRemoval.LastRemoved(removed)
            }
        }
        RostrumLog.i(TAG, "profile_removed", "profile" to id, "result" to removal::class.simpleName)
        return Outcome.Ok(removal)
    }

    /** Whether any signed-in profile has a notification toggle on (so the background check should run). */
    suspend fun wantsNotifications(): Boolean {
        val ready = _state.value as? ProfilesState.Ready ?: return false
        return ready.profiles.any { profile ->
            val handle = handle(profile.id)
            handle.session.state.value.isSignedIn &&
                handle.backend.settings().valueOrNull()?.let { it.notifyNewPullRequests || it.notifyReviewRequests } == true
        }
    }

    suspend fun parsePairingLink(uri: String): Outcome<PairingPreview> = registry.parsePairingLink(uri)

    suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> = registry.probeDesktop(host, port)

    private fun profileOrElse(fallback: Profile): Profile =
        (_state.value as? ProfilesState.Ready)?.profile(fallback.id) ?: fallback

    private fun <T> Outcome<T>.logErr(event: String): Outcome<T> = also {
        if (it is Outcome.Err) RostrumLog.w(TAG, event, "error" to it.error::class.simpleName)
    }

    companion object {
        /** A token profile's name until GitHub says whose token it is. */
        const val DEFAULT_TOKEN_LABEL = "GitHub"
        private const val TAG = "RostrumProfiles"
        private val NOT_LOADED = BackendError.Internal("the profiles are not loaded yet")
    }
}
