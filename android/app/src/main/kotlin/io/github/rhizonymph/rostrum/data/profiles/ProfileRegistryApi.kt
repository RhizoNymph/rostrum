package io.github.rhizonymph.rostrum.data.profiles

import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfilePairing

/**
 * The core's profile registry: which profiles exist, which one is active,
 * and one core (so one [RostrumBackend]) per profile, each with its own
 * data directory. It never sees secrets; [ProfileManager] keeps those.
 * The app talks to it only through [ProfileManager].
 */
interface ProfileRegistryApi {
    /** Most recently used first. */
    suspend fun profiles(): Outcome<List<Profile>>

    suspend fun activeProfile(): Outcome<ProfileId?>

    /** Make [id] the active profile (and its most recently used). */
    suspend fun setActiveProfile(id: ProfileId): Outcome<Profile>

    /**
     * The backend over [id]'s core. Building it contacts nothing; the core
     * opens on the first call. One per id, for the life of the process.
     */
    fun backend(id: ProfileId): RostrumBackend

    /** A new profile with no desktop, for a pasted GitHub token. Not made active. */
    suspend fun createTokenProfile(label: String): Outcome<Profile>

    /**
     * Pair from a `rostrum://pair?…` link: into the profile already paired
     * with that desktop's certificate, or a new one. Never changes the active
     * profile.
     */
    suspend fun pairDesktopWithLink(uri: String, deviceName: String): Outcome<ProfilePairing>

    /** As [pairDesktopWithLink], by address and code, pinned to [fingerprint]. */
    suspend fun pairDesktopManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<ProfilePairing>

    suspend fun renameProfile(id: ProfileId, label: String): Outcome<Profile>

    suspend fun setProfileLogin(id: ProfileId, login: String?): Outcome<Profile>

    /** Unpair best-effort, close the core and delete the profile's data. */
    suspend fun removeProfile(id: ProfileId): Outcome<Unit>

    /** Read a pairing link without contacting anything. Needs no profile. */
    suspend fun parsePairingLink(uri: String): Outcome<PairingPreview>

    /** Ask a desktop typed in by address who it is. Needs no profile. */
    suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe>
}
