package io.github.rhizonymph.rostrum.ui.settings

import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.testing.InMemorySecretStore

/** A valid-looking GitHub token the fake backend accepts. */
const val TEST_TOKEN = "ghp_abcdefghijklmnopqrstuvwxyz0123"

/** Secrets for a phone that is signed in and, optionally, paired with nymph-desk. */
fun accountSecrets(signedIn: Boolean = true, paired: Boolean = true): InMemorySecretStore = InMemorySecretStore(
    buildMap {
        if (signedIn) {
            put(SecretKey.GitHubToken, TEST_TOKEN)
            put(SecretKey.GitHubHost, "github.com")
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
    secrets: InMemorySecretStore = accountSecrets(),
): SessionRepository = SessionRepository(backend, secrets, "Pixel 9").also { it.restore() }
