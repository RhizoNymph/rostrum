package io.github.rhizonymph.rostrum.data.secrets

import io.github.rhizonymph.rostrum.data.model.ProfileId
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File

/** A reversible stand-in for the Keystore cipher: XOR with a fixed pad, iv = counter. */
private class XorCipher : SecretCipher {
    var failSeal: String? = null
    var failOpen: CipherOutcome.Failed? = null
    private var counter = 0

    override fun seal(plaintext: ByteArray): CipherOutcome<SealedBox> {
        failSeal?.let { return CipherOutcome.Failed(it, keyLost = false) }
        val iv = ByteArray(12) { (counter + it).toByte() }.also { counter++ }
        return CipherOutcome.Ok(SealedBox(iv, xor(plaintext, iv)))
    }

    override fun open(box: SealedBox): CipherOutcome<ByteArray> {
        failOpen?.let { return it }
        return CipherOutcome.Ok(xor(box.ciphertext, box.iv))
    }

    private fun xor(data: ByteArray, iv: ByteArray) = ByteArray(data.size) { (data[it].toInt() xor iv[it % iv.size].toInt() xor 0x5A).toByte() }
}

private val P1 = ProfileId.of("p1")!!
private val P2 = ProfileId.of("p2")!!

class SecretStorageTest {
    @Nested
    inner class Envelope {
        @Test
        fun `round trips a box`() {
            val box = SealedBox(ByteArray(12) { it.toByte() }, byteArrayOf(9, 8, 7, 6))
            val decoded = SecretEnvelope.decode(SecretEnvelope.encode(box))
            assertEquals(SecretEnvelope.Decoded.Box(box), decoded)
        }

        @Test
        fun `starts with the version and the iv length`() {
            val bytes = SecretEnvelope.encode(SealedBox(ByteArray(12), byteArrayOf(1)))
            assertEquals(SecretEnvelope.VERSION, bytes[0])
            assertEquals(12, bytes[1].toInt())
            assertEquals(2 + 12 + 1, bytes.size)
        }

        @Test
        fun `rejects short, unversioned and truncated input`() {
            assertInstanceOf(SecretEnvelope.Decoded.Malformed::class.java, SecretEnvelope.decode(byteArrayOf()))
            assertInstanceOf(SecretEnvelope.Decoded.Malformed::class.java, SecretEnvelope.decode(byteArrayOf(1)))
            assertInstanceOf(SecretEnvelope.Decoded.Malformed::class.java, SecretEnvelope.decode(byteArrayOf(2, 1, 0, 0)))
            assertInstanceOf(SecretEnvelope.Decoded.Malformed::class.java, SecretEnvelope.decode(byteArrayOf(1, 0, 5)))
            assertInstanceOf(SecretEnvelope.Decoded.Malformed::class.java, SecretEnvelope.decode(byteArrayOf(1, 12, 1, 2, 3)))
        }
    }

    @Nested
    inner class FileStore {
        @TempDir
        lateinit var dir: File

        private val cipher = XorCipher()
        private fun store() = EncryptedFileSecretStore(File(dir, "secrets"), cipher, Dispatchers.Unconfined)

        @Test
        fun `an unwritten secret is absent`() = runTest {
            assertEquals(SecretRead.Absent, store().read(P1, SecretKey.GitHubToken))
        }

        @Test
        fun `a written secret reads back and is not stored in plain text`() = runTest {
            val store = store()
            assertEquals(SecretWrite.Done, store.write(P1, SecretKey.GitHubToken, "ghp_secretvalue"))
            assertEquals(SecretRead.Present("ghp_secretvalue"), store.read(P1, SecretKey.GitHubToken))
            val onDisk = File(dir, "secrets/p1/github_token.sealed").readBytes()
            assertFalse(String(onDisk, Charsets.ISO_8859_1).contains("ghp_secretvalue"))
        }

        @Test
        fun `keys are stored independently`() = runTest {
            val store = store()
            store.write(P1, SecretKey.GitHubToken, "a")
            store.write(P1, SecretKey.DeviceToken, "b")
            assertEquals("a", store.read(P1, SecretKey.GitHubToken).valueOrNull())
            assertEquals("b", store.read(P1, SecretKey.DeviceToken).valueOrNull())
        }

        @Test
        fun `overwriting replaces the value and leaves no temp file`() = runTest {
            val store = store()
            store.write(P1, SecretKey.DesktopEndpoint, "one")
            store.write(P1, SecretKey.DesktopEndpoint, "two")
            assertEquals("two", store.read(P1, SecretKey.DesktopEndpoint).valueOrNull())
            assertEquals(listOf("desktop_endpoint.sealed"), File(dir, "secrets/p1").list()!!.toList())
        }

        @Test
        fun `delete removes the value and deleting again succeeds`() = runTest {
            val store = store()
            store.write(P1, SecretKey.GitHubToken, "x")
            assertEquals(SecretWrite.Done, store.delete(P1, SecretKey.GitHubToken))
            assertEquals(SecretRead.Absent, store.read(P1, SecretKey.GitHubToken))
            assertEquals(SecretWrite.Done, store.delete(P1, SecretKey.GitHubToken))
        }

        @Test
        fun `a corrupt file reads as Corrupt, not as a value`() = runTest {
            File(dir, "secrets/p1").mkdirs()
            File(dir, "secrets/p1/github_token.sealed").writeBytes(byteArrayOf(7, 7, 7))
            val read = store().read(P1, SecretKey.GitHubToken)
            assertInstanceOf(SecretRead.Failed::class.java, read)
            assertInstanceOf(SecretStoreError.Corrupt::class.java, (read as SecretRead.Failed).error)
        }

        @Test
        fun `a lost key reads as KeystoreUnavailable`() = runTest {
            val store = store()
            store.write(P1, SecretKey.GitHubToken, "x")
            cipher.failOpen = CipherOutcome.Failed("invalidated", keyLost = true)
            val read = store.read(P1, SecretKey.GitHubToken) as SecretRead.Failed
            assertInstanceOf(SecretStoreError.KeystoreUnavailable::class.java, read.error)
        }

        @Test
        fun `a failing seal writes nothing`() = runTest {
            val store = store()
            cipher.failSeal = "no key"
            val write = store.write(P1, SecretKey.GitHubToken, "x")
            assertInstanceOf(SecretWrite.Failed::class.java, write)
            assertEquals(SecretRead.Absent, store.read(P1, SecretKey.GitHubToken))
        }

        @Test
        fun `values survive a new store instance`() = runTest {
            store().write(P1, SecretKey.DeviceToken, "rdt_1")
            assertEquals("rdt_1", store().read(P1, SecretKey.DeviceToken).valueOrNull())
        }

        @Test
        fun `each profile keeps its own secrets`() = runTest {
            val store = store()
            store.write(P1, SecretKey.GitHubToken, "one")
            store.write(P2, SecretKey.GitHubToken, "two")
            assertEquals("one", store.read(P1, SecretKey.GitHubToken).valueOrNull())
            assertEquals("two", store.read(P2, SecretKey.GitHubToken).valueOrNull())
            assertEquals(SecretRead.Absent, store.read(P2, SecretKey.DeviceToken))
        }

        @Test
        fun `deleting a profile removes all of its secrets and nothing else`() = runTest {
            val store = store()
            store.write(P1, SecretKey.GitHubToken, "one")
            store.write(P1, SecretKey.DeviceToken, "rdt")
            store.write(P2, SecretKey.GitHubToken, "two")
            assertEquals(SecretWrite.Done, store.deleteProfile(P1))
            assertEquals(SecretRead.Absent, store.read(P1, SecretKey.GitHubToken))
            assertEquals(SecretRead.Absent, store.read(P1, SecretKey.DeviceToken))
            assertEquals("two", store.read(P2, SecretKey.GitHubToken).valueOrNull())
            assertEquals(listOf("p2"), File(dir, "secrets").list()!!.toList())
            assertEquals(SecretWrite.Done, store.deleteProfile(P1))
        }

        @Test
        fun `a profile's view reads and writes only that profile`() = runTest {
            val store = store()
            val view = store.forProfile(P2)
            view.write(SecretKey.DesktopEndpoint, "{}")
            assertEquals("{}", store.read(P2, SecretKey.DesktopEndpoint).valueOrNull())
            assertEquals(SecretRead.Absent, store.read(P1, SecretKey.DesktopEndpoint))
        }

        @Test
        fun `present values never print`() {
            assertTrue("rdt" !in SecretRead.Present("rdt_secret").toString())
            assertArrayEquals(byteArrayOf(1), byteArrayOf(1))
        }
    }
}
