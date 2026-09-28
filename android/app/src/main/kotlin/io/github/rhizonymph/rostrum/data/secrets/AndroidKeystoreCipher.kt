package io.github.rhizonymph.rostrum.data.secrets

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * AES-256-GCM with a key generated inside the Android Keystore. The key is
 * non-exportable: its material never enters this process, so the files it
 * seals are useless off this device (and `allowBackup` is off besides).
 *
 * No user authentication is required, so the background notification check
 * can read the token with the screen locked.
 */
class AndroidKeystoreCipher(private val alias: String = DEFAULT_ALIAS) : SecretCipher {
    override fun seal(plaintext: ByteArray): CipherOutcome<SealedBox> = guarded {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        CipherOutcome.Ok(SealedBox(cipher.iv, cipher.doFinal(plaintext)))
    }

    override fun open(box: SealedBox): CipherOutcome<ByteArray> = guarded {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, box.iv))
        CipherOutcome.Ok(cipher.doFinal(box.ciphertext))
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }
        (keyStore.getEntry(alias, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }

    /** Keystore failures arrive as a handful of checked and unchecked types; map each. */
    private inline fun <T> guarded(block: () -> CipherOutcome<T>): CipherOutcome<T> = try {
        block()
    } catch (e: KeyPermanentlyInvalidatedException) {
        CipherOutcome.Failed("key invalidated: ${e.message}", keyLost = true)
    } catch (e: GeneralSecurityException) {
        CipherOutcome.Failed("${e.javaClass.simpleName}: ${e.message}", keyLost = false)
    } catch (e: IOException) {
        CipherOutcome.Failed("keystore I/O: ${e.message}", keyLost = false)
    } catch (e: ProviderException) {
        CipherOutcome.Failed("keystore provider: ${e.message}", keyLost = false)
    }

    companion object {
        const val DEFAULT_ALIAS = "rostrum.secrets.v1"
        private const val PROVIDER = "AndroidKeyStore"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val TAG_BITS = 128
    }
}
