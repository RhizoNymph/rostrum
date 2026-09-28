package io.github.rhizonymph.rostrum.data.profiles

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File

class LegacyStateWipeTest {
    @TempDir
    lateinit var dir: File

    private val core get() = File(dir, "files/core")
    private val secrets get() = File(dir, "no_backup/secrets")
    private fun wipe() = LegacyStateWipe(core, secrets).wipeNow()

    @Test
    fun `removes the single-profile core and unkeyed secrets`() {
        File(core, "cache").mkdirs()
        File(core, "config.json").writeText("{}")
        secrets.mkdirs()
        listOf("github_token.sealed", "device_token.sealed", "desktop_endpoint.sealed", "github_token.tmp")
            .forEach { File(secrets, it).writeBytes(byteArrayOf(1, 2, 3)) }
        assertEquals(LegacyWipeReport(coreRemoved = true, secretsRemoved = 4), wipe())
        assertFalse(core.exists())
        assertEquals(emptyList<String>(), secrets.list()!!.toList())
    }

    @Test
    fun `keeps every profile's secrets beside them`() {
        File(secrets, "profiles/0123456789abcdef").mkdirs()
        File(secrets, "profiles/0123456789abcdef/github_token.sealed").writeBytes(byteArrayOf(1))
        File(secrets, "github_token.sealed").writeBytes(byteArrayOf(1))
        assertEquals(LegacyWipeReport(coreRemoved = false, secretsRemoved = 1), wipe())
        assertTrue(File(secrets, "profiles/0123456789abcdef/github_token.sealed").exists())
    }

    @Test
    fun `a second run finds nothing to do`() {
        File(core, "x").mkdirs()
        wipe()
        val again = wipe()
        assertFalse(again.removedAnything)
    }

    @Test
    fun `nothing there is nothing removed`() {
        assertEquals(LegacyWipeReport(coreRemoved = false, secretsRemoved = 0), wipe())
    }
}
