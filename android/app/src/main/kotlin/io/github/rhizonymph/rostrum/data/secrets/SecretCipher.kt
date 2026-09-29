package io.github.rhizonymph.rostrum.data.secrets

/** Ciphertext and the nonce it was sealed with. */
class SealedBox(val iv: ByteArray, val ciphertext: ByteArray) {
    override fun equals(other: Any?): Boolean =
        other is SealedBox && iv.contentEquals(other.iv) && ciphertext.contentEquals(other.ciphertext)

    override fun hashCode(): Int = 31 * iv.contentHashCode() + ciphertext.contentHashCode()
}

sealed interface CipherOutcome<out T> {
    data class Ok<out T>(val value: T) : CipherOutcome<T>

    /** [keyLost] means the key itself is gone or invalidated, not just this value. */
    data class Failed(val reason: String, val keyLost: Boolean) : CipherOutcome<Nothing>
}

/**
 * Authenticated encryption with a key the implementation never exposes. The
 * production one lives in the Android Keystore ([AndroidKeystoreCipher]);
 * tests substitute their own.
 */
interface SecretCipher {
    fun seal(plaintext: ByteArray): CipherOutcome<SealedBox>

    fun open(box: SealedBox): CipherOutcome<ByteArray>
}

/**
 * The on-disk format of one secret: `[version][iv length][iv][ciphertext]`.
 * The version byte lets a later build change the scheme and still recognise
 * (and discard) old files instead of misreading them.
 */
object SecretEnvelope {
    const val VERSION: Byte = 1

    sealed interface Decoded {
        data class Box(val box: SealedBox) : Decoded

        data class Malformed(val reason: String) : Decoded
    }

    fun encode(box: SealedBox): ByteArray {
        require(box.iv.size in 1..255) { "iv must be 1..255 bytes, was ${box.iv.size}" }
        return byteArrayOf(VERSION, box.iv.size.toByte()) + box.iv + box.ciphertext
    }

    fun decode(bytes: ByteArray): Decoded {
        if (bytes.size < 2) return Decoded.Malformed("too short (${bytes.size} bytes)")
        if (bytes[0] != VERSION) return Decoded.Malformed("unknown version ${bytes[0]}")
        val ivLength = bytes[1].toInt() and 0xFF
        if (ivLength == 0) return Decoded.Malformed("empty iv")
        if (bytes.size < 2 + ivLength + 1) return Decoded.Malformed("truncated (${bytes.size} bytes, iv $ivLength)")
        val iv = bytes.copyOfRange(2, 2 + ivLength)
        val ciphertext = bytes.copyOfRange(2 + ivLength, bytes.size)
        return Decoded.Box(SealedBox(iv, ciphertext))
    }
}
