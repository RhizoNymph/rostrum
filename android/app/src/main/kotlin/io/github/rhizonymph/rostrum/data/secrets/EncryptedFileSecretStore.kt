package io.github.rhizonymph.rostrum.data.secrets

import io.github.rhizonymph.rostrum.data.model.ProfileId
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

/**
 * One sealed file per secret, in one directory per profile under
 * [directory] (app-private storage): `<directory>/<profile id>/<name>.sealed`.
 * Writes go to a temp file and are renamed into place, so a crash never
 * leaves half a secret behind.
 */
class EncryptedFileSecretStore(
    private val directory: File,
    private val cipher: SecretCipher,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : SecretStore {
    private val mutex = Mutex()

    private fun dirOf(profile: ProfileId) = File(directory, profile.value)

    private fun fileOf(profile: ProfileId, key: SecretKey) = File(dirOf(profile), "${key.fileName}.sealed")

    override suspend fun read(profile: ProfileId, key: SecretKey): SecretRead = withContext(io) {
        mutex.withLock {
            val file = fileOf(profile, key)
            if (!file.exists()) return@withLock SecretRead.Absent
            val bytes = try {
                file.readBytes()
            } catch (e: IOException) {
                return@withLock SecretRead.Failed(SecretStoreError.Io(key, e.message ?: "read failed"))
            }
            when (val decoded = SecretEnvelope.decode(bytes)) {
                is SecretEnvelope.Decoded.Malformed -> SecretRead.Failed(SecretStoreError.Corrupt(key, decoded.reason))
                is SecretEnvelope.Decoded.Box -> when (val opened = cipher.open(decoded.box)) {
                    is CipherOutcome.Ok -> SecretRead.Present(opened.value.toString(Charsets.UTF_8))
                    is CipherOutcome.Failed ->
                        if (opened.keyLost) {
                            SecretRead.Failed(SecretStoreError.KeystoreUnavailable(opened.reason))
                        } else {
                            SecretRead.Failed(SecretStoreError.Corrupt(key, opened.reason))
                        }
                }
            }
        }
    }

    override suspend fun write(profile: ProfileId, key: SecretKey, value: String): SecretWrite = withContext(io) {
        mutex.withLock {
            when (val sealed = cipher.seal(value.toByteArray(Charsets.UTF_8))) {
                is CipherOutcome.Failed -> SecretWrite.Failed(SecretStoreError.KeystoreUnavailable(sealed.reason))
                is CipherOutcome.Ok -> replace(profile, key, SecretEnvelope.encode(sealed.value))
            }
        }
    }

    override suspend fun delete(profile: ProfileId, key: SecretKey): SecretWrite = withContext(io) {
        mutex.withLock {
            val file = fileOf(profile, key)
            if (!file.exists() || file.delete()) {
                SecretWrite.Done
            } else {
                SecretWrite.Failed(SecretStoreError.Io(key, "could not delete ${file.name}"))
            }
        }
    }

    override suspend fun deleteProfile(profile: ProfileId): SecretWrite = withContext(io) {
        mutex.withLock {
            val dir = dirOf(profile)
            if (!dir.exists() || dir.deleteRecursively()) {
                SecretWrite.Done
            } else {
                SecretWrite.Failed(SecretStoreError.Io(null, "could not delete the secrets of profile $profile"))
            }
        }
    }

    private fun replace(profile: ProfileId, key: SecretKey, bytes: ByteArray): SecretWrite {
        val dir = dirOf(profile)
        val target = fileOf(profile, key)
        val temp = File(dir, "${key.fileName}.tmp")
        return try {
            if (!dir.exists() && !dir.mkdirs()) {
                return SecretWrite.Failed(SecretStoreError.Io(key, "could not create ${dir.name}"))
            }
            temp.writeBytes(bytes)
            try {
                Files.move(temp.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE)
            } catch (e: AtomicMoveNotSupportedException) {
                Files.move(temp.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
            }
            SecretWrite.Done
        } catch (e: IOException) {
            temp.delete()
            SecretWrite.Failed(SecretStoreError.Io(key, e.message ?: "write failed"))
        }
    }
}
