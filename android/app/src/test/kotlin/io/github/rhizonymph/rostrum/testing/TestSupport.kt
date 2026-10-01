package io.github.rhizonymph.rostrum.testing

import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.secrets.ProfileSecrets
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.secrets.SecretRead
import io.github.rhizonymph.rostrum.data.secrets.SecretStore
import io.github.rhizonymph.rostrum.data.secrets.SecretStoreError
import io.github.rhizonymph.rostrum.data.secrets.SecretWrite
import io.github.rhizonymph.rostrum.notifications.BackgroundWork
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.jupiter.api.extension.AfterEachCallback
import org.junit.jupiter.api.extension.BeforeEachCallback
import org.junit.jupiter.api.extension.ExtensionContext
import java.time.Clock
import java.time.Instant
import java.time.ZoneOffset

/**
 * Replaces `Dispatchers.Main` (and so `viewModelScope`) with a test
 * dispatcher. Register with `@JvmField @RegisterExtension val main = MainDispatcherExtension()`
 * and run tests with `runTest(main.dispatcher) { … advanceUntilIdle() }` so the
 * test and the ViewModel share one virtual clock.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MainDispatcherExtension(
    val dispatcher: TestDispatcher = StandardTestDispatcher(),
) : BeforeEachCallback, AfterEachCallback {
    override fun beforeEach(context: ExtensionContext) = Dispatchers.setMain(dispatcher)

    override fun afterEach(context: ExtensionContext) = Dispatchers.resetMain()
}

/** A fixed instant for deterministic relative times: 2026-09-28T12:00:00Z. */
val TEST_NOW: Instant = Instant.parse("2026-09-28T12:00:00Z")
val TEST_CLOCK: Clock = Clock.fixed(TEST_NOW, ZoneOffset.UTC)

/** The default sorts (repositories pushed ↓, items created ↓), as a snapshot carries them. */
val TEST_SORT = io.github.rhizonymph.rostrum.data.fake.FakeSort().settings()

/** A signed-in, paired fake on the test clock with no latency. */
fun testBackend(signedIn: Boolean = true, paired: Boolean = true): FakeRostrumBackend =
    FakeRostrumBackend(clock = TEST_CLOCK, signedIn = signedIn, paired = paired)

/** The value of an [Outcome.Ok], failing the test with the error otherwise. */
fun <T> Outcome<T>.orFail(): T = when (this) {
    is Outcome.Ok -> value
    is Outcome.Err -> throw AssertionError("expected Ok, got $error")
}

/** One profile's secrets in a map, with per-key failure injection. */
class InMemoryProfileSecrets(initial: Map<SecretKey, String> = emptyMap()) : ProfileSecrets {
    val values = initial.toMutableMap()
    val failReads = mutableMapOf<SecretKey, SecretStoreError>()
    val failWrites = mutableMapOf<SecretKey, SecretStoreError>()

    /** Every read, in order (to check which profile was restored first). */
    val reads = mutableListOf<SecretKey>()

    override suspend fun read(key: SecretKey): SecretRead {
        reads += key
        failReads[key]?.let { return SecretRead.Failed(it) }
        return values[key]?.let { SecretRead.Present(it) } ?: SecretRead.Absent
    }

    override suspend fun write(key: SecretKey, value: String): SecretWrite {
        failWrites[key]?.let { return SecretWrite.Failed(it) }
        values[key] = value
        return SecretWrite.Done
    }

    override suspend fun delete(key: SecretKey): SecretWrite {
        values.remove(key)
        return SecretWrite.Done
    }
}

/** A [SecretStore] keeping one [InMemoryProfileSecrets] per profile. */
class InMemorySecretVault : SecretStore {
    val profiles = linkedMapOf<ProfileId, InMemoryProfileSecrets>()

    /** The profile of every read, in order. */
    val readOrder = mutableListOf<ProfileId>()

    fun of(profile: ProfileId): InMemoryProfileSecrets = profiles.getOrPut(profile) { InMemoryProfileSecrets() }

    fun seed(profile: ProfileId, values: Map<SecretKey, String>) {
        of(profile).values.putAll(values)
    }

    override suspend fun read(profile: ProfileId, key: SecretKey): SecretRead {
        readOrder += profile
        return of(profile).read(key)
    }

    override suspend fun write(profile: ProfileId, key: SecretKey, value: String): SecretWrite = of(profile).write(key, value)

    override suspend fun delete(profile: ProfileId, key: SecretKey): SecretWrite = of(profile).delete(key)

    override suspend fun deleteProfile(profile: ProfileId): SecretWrite {
        profiles.remove(profile)
        return SecretWrite.Done
    }
}

/** A profile id for tests; fails on a malformed one. */
fun pid(raw: String): ProfileId = requireNotNull(ProfileId.of(raw)) { "bad test profile id $raw" }

/** Records what the notification scheduler asked WorkManager to do. */
class RecordingBackgroundWork : BackgroundWork {
    val scheduled = mutableSetOf<String>()
    val calls = mutableListOf<String>()

    override fun schedulePeriodic(name: String) {
        scheduled += name
        calls += "schedule:$name"
    }

    override fun cancel(name: String) {
        scheduled -= name
        calls += "cancel:$name"
    }
}
