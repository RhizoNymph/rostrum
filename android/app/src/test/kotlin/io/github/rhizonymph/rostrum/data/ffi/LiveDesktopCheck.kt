package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.testing.orFail
import kotlinx.coroutines.runBlocking
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File

/**
 * Pairs with the `rostrumd` running on this machine, uses the pairing, and
 * unpairs. Never part of CI: run by hand with `./gradlew :app:liveDesktopCheck`
 * and `ROSTRUM_LIVE_PAIR_URI_FILE` naming a file that holds a fresh pairing
 * link. Writes a summary (no secrets) to `ROSTRUM_LIVE_RESULT_FILE` if set.
 * Tokens are only checked for presence, never printed.
 */
@Tag("live-desktop")
class LiveDesktopCheck {
    @TempDir
    lateinit var dir: File

    @Test
    fun `pairs with the local desktop, uses it, and unpairs`(): Unit = runBlocking {
        val uriFile = System.getenv("ROSTRUM_LIVE_PAIR_URI_FILE")
        assumeTrue(System.getProperty("rostrum.hostTests") == "true" && uriFile != null, "no live pairing link")
        val uri = File(uriFile!!).readText().trim()
        val backend = FfiRostrumBackend(File(dir, "core"))
        val lines = mutableListOf<String>()

        val preview = backend.parsePairingLink(uri).orFail()
        lines += "preview machine=${preview.machine} hosts=${preview.hosts.joinToString(",")} port=${preview.port} fingerprint=\"${preview.fingerprintShort}\""

        val paired = backend.pairWithLink(uri, DEVICE_NAME).orFail()
        assertFalse(paired.deviceToken.isBlank(), "device token missing")
        lines += "paired machine=${paired.machine.name} version=${paired.machine.version} device_id=${paired.deviceId} github_handed_over=${paired.github != null}"

        val info = backend.machineInfo().orFail()
        lines += "machine_info name=${info.name} api=${info.apiVersion} clones=${info.clones.size} handler=${info.handlerConfigured}"

        val handoffs = backend.handoffs().orFail()
        lines += "handoffs count=${handoffs.size}"

        lines += when (val token = backend.refreshGitHubTokenFromDesktop()) {
            is Outcome.Ok -> {
                assertFalse(token.value.token.isBlank(), "empty token")
                "github_token refreshed=true host=${token.value.host} source=\"${token.value.source}\""
            }
            is Outcome.Err -> "github_token refreshed=false error=${token.error::class.simpleName}"
        }

        backend.unpair().orFail()
        lines += "unpaired device_id=${paired.deviceId}"

        val summary = lines.joinToString("\n")
        System.getenv("ROSTRUM_LIVE_RESULT_FILE")?.let { File(it).writeText(summary + "\n") }
        println(summary)
    }

    private companion object {
        const val DEVICE_NAME = "rostrum-live-check"
    }
}
