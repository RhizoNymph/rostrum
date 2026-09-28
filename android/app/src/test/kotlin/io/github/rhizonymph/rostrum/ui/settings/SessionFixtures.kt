package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.testing.InMemoryProfileSecrets

/** A valid-looking GitHub token the fake backend accepts. */
const val TEST_TOKEN = "ghp_abcdefghijklmnopqrstuvwxyz0123"

/** Secrets for a phone that is signed in and, optionally, paired with nymph-desk. */
fun accountSecrets(signedIn: Boolean = true, paired: Boolean = true): InMemoryProfileSecrets = InMemoryProfileSecrets(
    buildMap {
        if (signedIn) {
            put(SecretKey.GitHubToken, TEST_TOKEN)
        }
        if (paired) {
            put(SecretKey.DesktopEndpoint, """{"hosts":["192.168.1.24","nymph-desk.local"],"port":8485}""")
            put(SecretKey.DeviceToken, "rdt_test")
        }
    },
)

/** A restored session over [backend] with [secrets]. */
suspend fun restoredSession(
    backend: RostrumBackend,
    secrets: InMemoryProfileSecrets = accountSecrets(),
): SessionRepository = SessionRepository(backend, secrets).also { it.restore() }
