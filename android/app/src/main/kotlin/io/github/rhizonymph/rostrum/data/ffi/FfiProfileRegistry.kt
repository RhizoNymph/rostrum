package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.model.ProfilePairing
import io.github.rhizonymph.rostrum.data.profiles.ProfileRegistryApi
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.rostrum_ffi.ProfileRegistry
import java.io.File
import java.time.Instant
import java.util.concurrent.ConcurrentHashMap
import uniffi.rostrum_ffi.ProfileInfo as FfiProfileInfo
import uniffi.rostrum_ffi.ProfileKind as FfiProfileKind
import uniffi.rostrum_ffi.ProfilePairing as FfiProfilePairing

/**
 * The core's `ProfileRegistry` behind [ProfileRegistryApi]. The registry is
 * opened on [rootDir] (`files/rostrum`: `profiles.json` and `profiles/<id>`) on the first call, off the main
 * thread; a failure to open is returned and retried by the next call. Its
 * blocking calls (the registry file) run on [io]. Each profile's backend
 * gets its core from [ProfileRegistry.core], which caches one per id.
 */
class FfiProfileRegistry(
    private val rootDir: File,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ProfileRegistryApi {
    private val openLock = Mutex()

    @Volatile
    private var registry: ProfileRegistry? = null

    private val backends = ConcurrentHashMap<ProfileId, RostrumBackend>()

    private suspend fun registry(): Outcome<ProfileRegistry> {
        registry?.let { return Outcome.Ok(it) }
        return openLock.withLock {
            registry?.let { return@withLock Outcome.Ok(it) }
            val opened = withContext(io) {
                CoreHandle.withNativeLibrary {
                    if (!rootDir.isDirectory && !rootDir.mkdirs()) {
                        Outcome.Err(BackendError.Storage("could not create ${rootDir.path}"))
                    } else {
                        ffiCall("openRegistry") { ProfileRegistry.open(rootDir.absolutePath) }
                    }
                }
            }
            if (opened is Outcome.Ok) {
                registry = opened.value
                RostrumLog.i(TAG, "registry_opened")
            }
            opened
        }
    }

    /** One registry call on [io], its errors mapped. */
    private suspend inline fun <T> call(op: String, crossinline block: suspend (ProfileRegistry) -> T): Outcome<T> =
        when (val opened = registry()) {
            is Outcome.Err -> opened
            is Outcome.Ok -> withContext(io) { ffiCall(op) { block(opened.value) } }
        }

    override suspend fun profiles(): Outcome<List<Profile>> =
        call("profiles") { registry -> registry.profiles().mapNotNull { it.toModelOrNull() } }

    override suspend fun activeProfile(): Outcome<ProfileId?> =
        call("activeProfile") { it.activeProfile()?.let(ProfileId::of) }

    override suspend fun setActiveProfile(id: ProfileId): Outcome<Profile> =
        call("setActiveProfile") { it.setActiveProfile(id.value) }.toProfile()

    override fun backend(id: ProfileId): RostrumBackend = backends.computeIfAbsent(id) {
        FfiRostrumBackend(id.value) {
            when (val opened = registry()) {
                is Outcome.Err -> opened
                is Outcome.Ok -> ffiCall("core") { opened.value.core(id.value) }
            }
        }
    }

    override suspend fun createTokenProfile(label: String): Outcome<Profile> =
        call("createTokenProfile") { it.createTokenProfile(label) }.toProfile()

    override suspend fun pairDesktopWithLink(uri: String, deviceName: String): Outcome<ProfilePairing> =
        call("pairDesktopWithLink") { it.pairDesktopWithLink(uri, deviceName) }.toPairing()

    override suspend fun pairDesktopManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<ProfilePairing> =
        if (port !in 1..65535) {
            Outcome.Err(BackendError.InvalidInput("Ports run from 1 to 65535"))
        } else {
            call("pairDesktopManual") { it.pairDesktopManual(host, port.toUShort(), fingerprint, code, deviceName) }.toPairing()
        }

    override suspend fun renameProfile(id: ProfileId, label: String): Outcome<Profile> =
        call("renameProfile") { it.renameProfile(id.value, label) }.toProfile()

    override suspend fun setProfileLogin(id: ProfileId, login: String?): Outcome<Profile> =
        call("setProfileLogin") { it.setProfileLogin(id.value, login) }.toProfile()

    override suspend fun removeProfile(id: ProfileId): Outcome<Unit> {
        val removed = call("removeProfile") { it.removeProfile(id.value) }
        if (removed is Outcome.Ok) backends.remove(id)
        return removed
    }

    override suspend fun parsePairingLink(uri: String): Outcome<PairingPreview> =
        call("parsePairingLink") { it.parsePairingLink(uri).toModel() }

    override suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> =
        if (port !in 1..65535) {
            Outcome.Err(BackendError.InvalidInput("Ports run from 1 to 65535"))
        } else {
            call("probeDesktop") { it.probeDesktop(host, port.toUShort()).toModel() }
        }

    private fun Outcome<FfiProfileInfo>.toProfile(): Outcome<Profile> = when (this) {
        is Outcome.Err -> this
        is Outcome.Ok -> value.toModel()
    }

    private fun Outcome<FfiProfilePairing>.toPairing(): Outcome<ProfilePairing> = when (this) {
        is Outcome.Err -> this
        is Outcome.Ok -> value.toModel()
    }

    private companion object {
        const val TAG = "RostrumProfiles"
    }
}

/* Records. The registry makes ids of 16 hex characters; any other shape is refused rather than used as a path. */

internal fun FfiProfileKind.toModel(): ProfileKind = when (this) {
    is FfiProfileKind.Desktop -> ProfileKind.Desktop(machine, fingerprintShort)
    FfiProfileKind.TokenOnly -> ProfileKind.TokenOnly
}

internal fun FfiProfileInfo.toModelOrNull(): Profile? {
    val profileId = ProfileId.of(id)
    if (profileId == null) {
        RostrumLog.w("RostrumProfiles", "profile_id_malformed")
        return null
    }
    return Profile(
        id = profileId,
        label = label,
        kind = kind.toModel(),
        githubLogin = githubLogin,
        createdAt = Instant.ofEpochMilli(createdAtMs),
        lastUsed = Instant.ofEpochMilli(lastUsedMs),
    )
}

internal fun FfiProfileInfo.toModel(): Outcome<Profile> =
    toModelOrNull()?.let { Outcome.Ok(it) } ?: Outcome.Err(BackendError.Internal("the registry returned a malformed profile id"))

internal fun FfiProfilePairing.toModel(): Outcome<ProfilePairing> = when (val mapped = profile.toModel()) {
    is Outcome.Err -> mapped
    is Outcome.Ok -> Outcome.Ok(ProfilePairing(mapped.value, created, pairing.toModel()))
}
