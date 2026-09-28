package io.github.rhizonymph.rostrum.data.profiles

import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/** Clears what the single-profile builds left behind. Runs before the registry opens. */
fun interface LegacyCleanup {
    /** Whether anything was there to wipe (this start is the first after an upgrade). */
    suspend fun wipe(): Boolean

    companion object {
        val None = LegacyCleanup { false }
    }
}

/** What [LegacyStateWipe] found and removed. */
data class LegacyWipeReport(val coreRemoved: Boolean, val secretsRemoved: Int) {
    val removedAnything: Boolean get() = coreRemoved || secretsRemoved > 0
}

/**
 * The single-profile state, dropped without migration: the core's data
 * directory ([legacyCoreDir], `files/core`) and the unkeyed secrets directly
 * in [legacySecretsDir]. Only the files those builds wrote are touched, so
 * the per-profile secrets beside them survive; running it again finds
 * nothing to do.
 */
class LegacyStateWipe(
    private val legacyCoreDir: File,
    private val legacySecretsDir: File,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : LegacyCleanup {
    override suspend fun wipe(): Boolean {
        val report = withContext(io) { wipeNow() }
        if (report.removedAnything) {
            RostrumLog.i(TAG, "legacy_state_wiped", "core" to report.coreRemoved, "secrets" to report.secretsRemoved)
        }
        return report.removedAnything
    }

    fun wipeNow(): LegacyWipeReport {
        val coreRemoved = legacyCoreDir.exists() && legacyCoreDir.deleteRecursively()
        if (legacyCoreDir.exists()) RostrumLog.w(TAG, "legacy_core_not_removed")
        val legacyFiles = SecretKey.entries.flatMap { key ->
            listOf(File(legacySecretsDir, "${key.fileName}.sealed"), File(legacySecretsDir, "${key.fileName}.tmp"))
        }
        val secretsRemoved = legacyFiles.count { it.isFile && it.delete() }
        return LegacyWipeReport(coreRemoved, secretsRemoved)
    }

    private companion object {
        const val TAG = "RostrumProfiles"
    }
}
