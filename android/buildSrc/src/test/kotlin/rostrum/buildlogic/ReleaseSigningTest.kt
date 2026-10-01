package rostrum.buildlogic

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File

class ReleaseSigningTest {
    @TempDir
    lateinit var dir: File

    private val noEnvironment: (String) -> String? = { null }

    private fun keystore(): File = dir.resolve("release.jks").apply { writeText("not a real keystore") }

    private fun dotEnv(text: String): File = dir.resolve(".env").apply { writeText(text) }

    private fun completeEnv(keystore: File) = """
        ROSTRUM_KEYSTORE=${keystore.absolutePath}
        ROSTRUM_KEYSTORE_PASSWORD=store-secret
        ROSTRUM_KEY_ALIAS=rostrum
        ROSTRUM_KEY_PASSWORD=key-secret
    """.trimIndent()

    @Test
    fun `parseDotEnv skips blanks and comments and keeps values verbatim`() {
        val parsed = ReleaseSigning.parseDotEnv(
            """
            # a comment

            PLAIN=value
            export EXPORTED=yes
            DOUBLE="quoted value"
            SINGLE='single'
            WITH_EQUALS=a=b=c
              SPACED  =  padded
            not a setting
            =no-key
            """.trimIndent(),
        )
        assertEquals(
            mapOf(
                "PLAIN" to "value",
                "EXPORTED" to "yes",
                "DOUBLE" to "quoted value",
                "SINGLE" to "single",
                "WITH_EQUALS" to "a=b=c",
                "SPACED" to "padded",
            ),
            parsed,
        )
    }

    @Test
    fun `parseDotEnv leaves unbalanced quotes alone`() {
        assertEquals(mapOf("A" to "\"open"), ReleaseSigning.parseDotEnv("A=\"open"))
    }

    @Test
    fun `complete dot env yields the keystore`() {
        val ks = keystore()
        val signing = ReleaseSigning.load(dotEnv(completeEnv(ks)), noEnvironment)
        assertEquals(ReleaseSigning.Keystore(ks, "store-secret", "rostrum", "key-secret"), signing)
    }

    @Test
    fun `environment takes precedence over the dot env`() {
        val ks = keystore()
        val env = mapOf("ROSTRUM_KEY_ALIAS" to "from-env")
        val signing = ReleaseSigning.load(dotEnv(completeEnv(ks)), env::get)
        assertEquals("from-env", assertInstanceOf(ReleaseSigning.Keystore::class.java, signing).keyAlias)
    }

    @Test
    fun `environment alone is enough`() {
        val ks = keystore()
        val env = mapOf(
            "ROSTRUM_KEYSTORE" to ks.absolutePath,
            "ROSTRUM_KEYSTORE_PASSWORD" to "s",
            "ROSTRUM_KEY_ALIAS" to "a",
            "ROSTRUM_KEY_PASSWORD" to "k",
        )
        val signing = ReleaseSigning.load(dir.resolve("missing.env"), env::get)
        assertInstanceOf(ReleaseSigning.Keystore::class.java, signing)
    }

    @Test
    fun `missing dot env and environment is unavailable and names every setting`() {
        val signing = ReleaseSigning.load(dir.resolve("missing.env"), noEnvironment)
        val reason = assertInstanceOf(ReleaseSigning.Unavailable::class.java, signing).reason
        listOf("ROSTRUM_KEYSTORE", "ROSTRUM_KEYSTORE_PASSWORD", "ROSTRUM_KEY_ALIAS", "ROSTRUM_KEY_PASSWORD")
            .forEach { assertTrue(it in reason, "reason should name $it: $reason") }
        assertTrue("does not exist" in reason, reason)
    }

    @Test
    fun `an empty value counts as missing`() {
        val ks = keystore()
        val text = completeEnv(ks).replace("ROSTRUM_KEY_PASSWORD=key-secret", "ROSTRUM_KEY_PASSWORD=")
        val signing = ReleaseSigning.load(dotEnv(text), noEnvironment)
        assertEquals(
            ReleaseSigning.Unavailable("ROSTRUM_KEY_PASSWORD not set in ${dir.resolve(".env")} or the environment"),
            signing,
        )
    }

    @Test
    fun `a keystore path that does not exist is unavailable`() {
        val gone = dir.resolve("gone.jks")
        val signing = ReleaseSigning.load(dotEnv(completeEnv(gone)), noEnvironment)
        assertEquals(ReleaseSigning.Unavailable("keystore $gone does not exist"), signing)
    }

    @Test
    fun `tilde in the keystore path is the home directory`() {
        val home = File(System.getProperty("user.home"))
        val env = mapOf(
            "ROSTRUM_KEYSTORE" to "~/definitely-not-a-rostrum-keystore.jks",
            "ROSTRUM_KEYSTORE_PASSWORD" to "s",
            "ROSTRUM_KEY_ALIAS" to "a",
            "ROSTRUM_KEY_PASSWORD" to "k",
        )
        val signing = ReleaseSigning.load(dir.resolve("missing.env"), env::get)
        assertEquals(
            ReleaseSigning.Unavailable("keystore ${home.resolve("definitely-not-a-rostrum-keystore.jks")} does not exist"),
            signing,
        )
    }

    @Test
    fun `toString never includes the passwords`() {
        val text = ReleaseSigning.Keystore(File("/k.jks"), "store-secret", "rostrum", "key-secret").toString()
        assertFalse("store-secret" in text || "key-secret" in text, text)
    }
}
