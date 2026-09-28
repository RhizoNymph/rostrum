package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfilePairing
import io.github.rhizonymph.rostrum.data.profiles.ProfileRegistryApi
import java.io.File

/*
 * The core's `ProfileRegistry` (opened on [rootDir]) behind
 * [ProfileRegistryApi]. The generated bindings gain it with the next core
 * merge; until then every call answers this error, and the mapping of
 * `ProfileInfo`, `ProfileKind` and `ProfilePairing` lands here with them.
 */
private fun <T> profilesUnavailable(): Outcome<T> =
    Outcome.Err(BackendError.Internal("profiles need a newer core"))

class FfiProfileRegistry(@Suppress("unused") private val rootDir: File) : ProfileRegistryApi {
    override suspend fun profiles(): Outcome<List<Profile>> = profilesUnavailable()

    override suspend fun activeProfile(): Outcome<ProfileId?> = profilesUnavailable()

    override suspend fun setActiveProfile(id: ProfileId): Outcome<Profile> = profilesUnavailable()

    override fun backend(id: ProfileId): RostrumBackend = FfiRostrumBackend(id.value) { profilesUnavailable() }

    override suspend fun createTokenProfile(label: String): Outcome<Profile> = profilesUnavailable()

    override suspend fun pairDesktopWithLink(uri: String, deviceName: String): Outcome<ProfilePairing> =
        profilesUnavailable()

    override suspend fun pairDesktopManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<ProfilePairing> = profilesUnavailable()

    override suspend fun renameProfile(id: ProfileId, label: String): Outcome<Profile> = profilesUnavailable()

    override suspend fun setProfileLogin(id: ProfileId, login: String?): Outcome<Profile> = profilesUnavailable()

    override suspend fun removeProfile(id: ProfileId): Outcome<Unit> = profilesUnavailable()

    override suspend fun parsePairingLink(uri: String): Outcome<PairingPreview> = profilesUnavailable()

    override suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> = profilesUnavailable()
}
