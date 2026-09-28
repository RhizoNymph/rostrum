package io.github.rhizonymph.rostrum.data.secrets

import io.github.rhizonymph.rostrum.data.model.ProfileId

/** The secrets each profile keeps between launches. [fileName] is the on-disk name. */
enum class SecretKey(val fileName: String) {
    /** The GitHub token: pasted, or handed over by the desktop. */
    GitHubToken("github_token"),

    /** The paired desktop's bearer credential for this device. */
    DeviceToken("device_token"),

    /** The paired desktop's addresses, port and certificate fingerprint (JSON from the core). */
    DesktopEndpoint("desktop_endpoint"),
}

/** Why reading or writing a secret failed. */
sealed interface SecretStoreError {
    /** The Keystore key could not be created or used (lock-screen change, broken provider). */
    data class KeystoreUnavailable(val reason: String) : SecretStoreError

    /** The stored bytes are not a sealed box this build can open; the value is lost. */
    data class Corrupt(val key: SecretKey, val reason: String) : SecretStoreError

    /** The file could not be read, written or replaced. [key] is `null` for a whole profile. */
    data class Io(val key: SecretKey?, val reason: String) : SecretStoreError
}

fun SecretStoreError.describe(): String = when (this) {
    is SecretStoreError.KeystoreUnavailable -> "The Android Keystore is unavailable: $reason"
    is SecretStoreError.Corrupt -> "The saved ${key.fileName} can't be read: $reason"
    is SecretStoreError.Io -> "Couldn't access the saved ${key?.fileName ?: "secrets"}: $reason"
}

/** The result of reading one secret. */
sealed interface SecretRead {
    data class Present(val value: String) : SecretRead {
        override fun toString(): String = "Present(value=redacted)"
    }

    data object Absent : SecretRead

    data class Failed(val error: SecretStoreError) : SecretRead
}

/** The result of writing or deleting one secret. */
sealed interface SecretWrite {
    data object Done : SecretWrite

    data class Failed(val error: SecretStoreError) : SecretWrite
}

/**
 * One profile's secrets. The session of that profile reads and writes
 * through this; it cannot reach another profile's. Nothing here is ever
 * logged. All calls are main-safe.
 */
interface ProfileSecrets {
    suspend fun read(key: SecretKey): SecretRead

    suspend fun write(key: SecretKey, value: String): SecretWrite

    /** Deleting an absent secret succeeds. */
    suspend fun delete(key: SecretKey): SecretWrite
}

/**
 * Secrets at rest, kept per profile. Implementations encrypt every value;
 * nothing here is ever logged. All calls are main-safe.
 */
interface SecretStore {
    suspend fun read(profile: ProfileId, key: SecretKey): SecretRead

    suspend fun write(profile: ProfileId, key: SecretKey, value: String): SecretWrite

    /** Deleting an absent secret succeeds. */
    suspend fun delete(profile: ProfileId, key: SecretKey): SecretWrite

    /** Every secret of [profile]; deleting a profile with none succeeds. */
    suspend fun deleteProfile(profile: ProfileId): SecretWrite
}

/** [profile]'s slice of this store. */
fun SecretStore.forProfile(profile: ProfileId): ProfileSecrets = object : ProfileSecrets {
    override suspend fun read(key: SecretKey) = this@forProfile.read(profile, key)

    override suspend fun write(key: SecretKey, value: String) = this@forProfile.write(profile, key, value)

    override suspend fun delete(key: SecretKey) = this@forProfile.delete(profile, key)
}

fun SecretRead.valueOrNull(): String? = (this as? SecretRead.Present)?.value
