package io.github.rhizonymph.rostrum.testing

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.model.PairingResult
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.model.ProfilePairing
import io.github.rhizonymph.rostrum.data.profiles.LegacyCleanup
import io.github.rhizonymph.rostrum.data.profiles.ProfileManager
import io.github.rhizonymph.rostrum.data.profiles.ProfileRegistryApi
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope

/**
 * The core's profile registry in memory, over one [FakeRostrumBackend] per
 * profile. A desktop is known by the short fingerprint its link or probe
 * shows, as the core knows it by the full one: pairing the same desktop
 * again re-pairs its profile. Ids are `p1`, `p2`, … in creation order.
 */
class FakeProfileRegistry(
    private val newBackend: (ProfileId) -> FakeRostrumBackend = { testBackend(signedIn = false, paired = false) },
) : ProfileRegistryApi {
    private class Entry(var profile: Profile, val fingerprint: String?)

    private val entries = mutableListOf<Entry>()
    val backends = linkedMapOf<ProfileId, FakeRostrumBackend>()
    /** Setting it also makes that profile the most recently used, as the core does. */
    var active: ProfileId? = null
        set(value) {
            field = value
            value?.let(::entry)?.let { it.profile = it.profile.copy(lastUsed = now()) }
        }
    private var tick = 0L
    private var nextId = 1

    /** Removed profiles, in order. */
    val removed = mutableListOf<ProfileId>()

    /** Every registry operation, by name. */
    val calls = mutableListOf<String>()
    private val failures = mutableMapOf<String, BackendError>()

    /** A scratch backend for reading links and probing, as the core does without a profile. */
    private val scratch = testBackend(signedIn = false, paired = false)

    fun failNext(op: String, error: BackendError) {
        failures[op] = error
    }

    private inline fun <T> call(op: String, block: () -> Outcome<T>): Outcome<T> {
        calls += op
        failures.remove(op)?.let { return Outcome.Err(it) }
        return block()
    }

    private fun now() = TEST_NOW.plusSeconds(tick++)

    private fun entry(id: ProfileId) = entries.firstOrNull { it.profile.id == id }

    private fun notFound(id: ProfileId) = Outcome.Err(BackendError.ProfileNotFound(id.value))

    private fun create(label: String, kind: ProfileKind, fingerprint: String?, login: String? = null): Entry {
        val at = now()
        val profile = Profile(pid("p${nextId++}"), label, kind, login, createdAt = at, lastUsed = at)
        return Entry(profile, fingerprint).also { entries += it }
    }

    /** A profile made before the test starts; its backend from [backend] or [newBackend]. */
    fun seed(
        label: String,
        kind: ProfileKind = ProfileKind.Desktop(label, "4F2A · 91C0 · 7E3B"),
        login: String? = null,
        backend: FakeRostrumBackend? = null,
    ): ProfileId {
        val fingerprint = (kind as? ProfileKind.Desktop)?.fingerprintShort
        val id = create(label, kind, fingerprint, login).profile.id
        backend?.let { backends[id] = it }
        return id
    }

    fun profile(id: ProfileId): Profile? = entry(id)?.profile

    override suspend fun profiles(): Outcome<List<Profile>> = call("profiles") {
        Outcome.Ok(entries.map { it.profile }.sortedWith(compareByDescending<Profile> { it.lastUsed }.thenByDescending { it.createdAt }))
    }

    override suspend fun activeProfile(): Outcome<ProfileId?> = call("activeProfile") { Outcome.Ok(active) }

    override suspend fun setActiveProfile(id: ProfileId): Outcome<Profile> = call("setActiveProfile") {
        val found = entry(id) ?: return@call notFound(id)
        found.profile = found.profile.copy(lastUsed = now())
        active = id
        Outcome.Ok(found.profile)
    }

    override fun backend(id: ProfileId): FakeRostrumBackend = backends.getOrPut(id) { newBackend(id) }

    override suspend fun createTokenProfile(label: String): Outcome<Profile> = call("createTokenProfile") {
        Outcome.Ok(create(label, ProfileKind.TokenOnly, fingerprint = null).profile)
    }

    override suspend fun pairDesktopWithLink(uri: String, deviceName: String): Outcome<ProfilePairing> =
        call("pairDesktopWithLink") {
            when (val preview = scratch.parsePairingLink(uri)) {
                is Outcome.Err -> preview
                is Outcome.Ok -> pairInto(preview.value.machine, preview.value.fingerprintShort) { backend(it).pairWithLink(uri, deviceName) }
            }
        }

    override suspend fun pairDesktopManual(
        host: String,
        port: Int,
        fingerprint: String,
        code: String,
        deviceName: String,
    ): Outcome<ProfilePairing> = call("pairDesktopManual") {
        when (val probe = scratch.probeDesktop(host, port)) {
            is Outcome.Err -> probe
            is Outcome.Ok -> pairInto(probe.value.machine, probe.value.fingerprintShort) {
                backend(it).pairManual(host, port, fingerprint, code, deviceName)
            }
        }
    }

    private suspend fun pairInto(
        machine: String,
        fingerprint: String,
        pair: suspend (ProfileId) -> Outcome<PairingResult>,
    ): Outcome<ProfilePairing> {
        val existing = entries.firstOrNull { it.fingerprint == fingerprint }
        val target = existing ?: create(machine, ProfileKind.Desktop(machine, fingerprint), fingerprint)
        return when (val paired = pair(target.profile.id)) {
            is Outcome.Err -> {
                if (existing == null) entries.remove(target)
                paired
            }
            is Outcome.Ok -> Outcome.Ok(ProfilePairing(target.profile, created = existing == null, pairing = paired.value))
        }
    }

    override suspend fun renameProfile(id: ProfileId, label: String): Outcome<Profile> = call("renameProfile") {
        val found = entry(id) ?: return@call notFound(id)
        found.profile = found.profile.copy(label = label)
        Outcome.Ok(found.profile)
    }

    override suspend fun setProfileLogin(id: ProfileId, login: String?): Outcome<Profile> = call("setProfileLogin") {
        val found = entry(id) ?: return@call notFound(id)
        found.profile = found.profile.copy(githubLogin = login)
        Outcome.Ok(found.profile)
    }

    override suspend fun removeProfile(id: ProfileId): Outcome<Unit> = call("removeProfile") {
        val found = entry(id) ?: return@call notFound(id)
        entries.remove(found)
        backends.remove(id)
        removed += id
        if (active == id) active = null
        Outcome.Ok(Unit)
    }

    override suspend fun parsePairingLink(uri: String): Outcome<PairingPreview> = call("parsePairingLink") {
        scratch.parsePairingLink(uri)
    }

    override suspend fun probeDesktop(host: String, port: Int): Outcome<DesktopProbe> = call("probeDesktop") {
        scratch.probeDesktop(host, port)
    }
}

/**
 * A [ProfileManager] over [registry] and [secrets]. Its watchers run on the
 * test's scheduler but outside the test's job (they never finish), and not
 * in `backgroundScope`, whose work `advanceUntilIdle` does not wait for.
 */
fun TestScope.testProfileManager(
    registry: FakeProfileRegistry,
    secrets: InMemorySecretVault,
    legacy: LegacyCleanup = LegacyCleanup.None,
): ProfileManager {
    val scope = CoroutineScope(StandardTestDispatcher(testScheduler) + SupervisorJob())
    return ProfileManager(registry, secrets, "Pixel 9", scope, legacy)
}
