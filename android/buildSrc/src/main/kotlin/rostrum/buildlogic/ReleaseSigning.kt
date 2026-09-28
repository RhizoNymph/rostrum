package rostrum.buildlogic

import java.io.File

/**
 * Where release builds get their signing key. The keystore itself lives
 * outside the repository; only its location and passwords are configured, in
 * the environment or in the gitignored `android/.env` (see `.env.example`).
 */
sealed interface ReleaseSigning {
    data class Keystore(
        val storeFile: File,
        val storePassword: String,
        val keyAlias: String,
        val keyPassword: String,
    ) : ReleaseSigning {
        override fun toString() = "Keystore(storeFile=$storeFile, keyAlias=$keyAlias)"
    }

    /** Release builds fall back to the debug key; [reason] says why. */
    data class Unavailable(val reason: String) : ReleaseSigning

    companion object {
        const val KEYSTORE = "ROSTRUM_KEYSTORE"
        const val KEYSTORE_PASSWORD = "ROSTRUM_KEYSTORE_PASSWORD"
        const val KEY_ALIAS = "ROSTRUM_KEY_ALIAS"
        const val KEY_PASSWORD = "ROSTRUM_KEY_PASSWORD"

        /**
         * Reads the four `ROSTRUM_*` settings, each from [environment] if set
         * there, else from [dotEnv]. A missing file, setting, or keystore is
         * [Unavailable], never an exception.
         */
        fun load(dotEnv: File, environment: (String) -> String?): ReleaseSigning {
            val fromFile = if (dotEnv.isFile) parseDotEnv(dotEnv.readText()) else emptyMap()
            fun setting(name: String): String? =
                environment(name)?.takeIf { it.isNotEmpty() } ?: fromFile[name]?.takeIf { it.isNotEmpty() }

            val missing = listOf(KEYSTORE, KEYSTORE_PASSWORD, KEY_ALIAS, KEY_PASSWORD).filter { setting(it) == null }
            if (missing.isNotEmpty()) {
                val where = if (dotEnv.isFile) "$dotEnv or the environment" else "the environment ($dotEnv does not exist)"
                return Unavailable("${missing.joinToString()} not set in $where")
            }
            val storeFile = expandHome(setting(KEYSTORE)!!)
            if (!storeFile.isFile) return Unavailable("keystore $storeFile does not exist")
            return Keystore(
                storeFile = storeFile,
                storePassword = setting(KEYSTORE_PASSWORD)!!,
                keyAlias = setting(KEY_ALIAS)!!,
                keyPassword = setting(KEY_PASSWORD)!!,
            )
        }

        /**
         * `KEY=VALUE` lines; blank lines and `#` comments are skipped, an
         * `export ` prefix and one pair of surrounding quotes are dropped.
         */
        fun parseDotEnv(text: String): Map<String, String> =
            text.lineSequence()
                .map { it.trim() }
                .filter { it.isNotEmpty() && !it.startsWith("#") }
                .map { it.removePrefix("export ").trimStart() }
                .mapNotNull { line ->
                    val eq = line.indexOf('=')
                    if (eq <= 0) return@mapNotNull null
                    line.substring(0, eq).trim() to unquote(line.substring(eq + 1).trim())
                }
                .toMap()

        private fun unquote(value: String): String =
            if (value.length >= 2 && value.first() == value.last() && value.first() in "\"'") {
                value.substring(1, value.length - 1)
            } else {
                value
            }

        private fun expandHome(path: String): File =
            if (path == "~" || path.startsWith("~/")) {
                File(System.getProperty("user.home") + path.removePrefix("~"))
            } else {
                File(path)
            }
    }
}
