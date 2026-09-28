package io.github.rhizonymph.rostrum.data.session

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.GitHubStatus
import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.secrets.SecretStoreError
import io.github.rhizonymph.rostrum.testing.InMemorySecretStore
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class SessionRepositoryTest {
    private val goodToken = "ghp_abcdefghijklmnopqrstuvwxyz0123"

    private fun ready(repo: SessionRepository) = repo.state.value as SessionState.Ready

    @Test
    fun `starts restoring, then signed out and unpaired with no secrets`() = runTest {
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), InMemorySecretStore(), "Pixel")
        assertEquals(SessionState.Restoring, repo.state.value)
        repo.restore()
        assertEquals(SessionState.Ready(GitHubAuth.SignedOut(), DesktopLink.NotPaired), repo.state.value)
    }

    @Test
    fun `restore hands the saved token to the backend`() = runTest {
        val backend = testBackend(signedIn = false, paired = false)
        val secrets = InMemorySecretStore(mapOf(SecretKey.GitHubToken to goodToken))
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        assertEquals(GitHubAuth.SignedIn, ready(repo).github)
        assertEquals(GitHubStatus.Unverified, backend.githubStatus())
    }

    @Test
    fun `restore sets the saved desktop as the remote`() = runTest {
        val backend = testBackend(signedIn = false, paired = false)
        val secrets = InMemorySecretStore(
            mapOf(SecretKey.DesktopEndpoint to """{"hosts":["10.0.0.5"],"port":8485}""", SecretKey.DeviceToken to "rdt_1"),
        )
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        val desktop = ready(repo).desktop as DesktopLink.Paired
        assertEquals(listOf("10.0.0.5"), desktop.status.hosts)
        assertInstanceOf(RemoteStatus.Paired::class.java, backend.remoteStatus().orFail())
    }

    @Test
    fun `half a pairing is discarded`() = runTest {
        val secrets = InMemorySecretStore(mapOf(SecretKey.DeviceToken to "rdt_1"))
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        assertEquals(DesktopLink.NotPaired, ready(repo).desktop)
        assertFalse(SecretKey.DeviceToken in secrets.values)
    }

    @Test
    fun `an unreadable token signs out with a notice and is deleted`() = runTest {
        val secrets = InMemorySecretStore(mapOf(SecretKey.GitHubToken to goodToken))
        secrets.failReads[SecretKey.GitHubToken] = SecretStoreError.KeystoreUnavailable("gone")
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        val github = ready(repo).github as GitHubAuth.SignedOut
        assertTrue(github.notice!!.contains("Sign in again"))
        assertFalse(SecretKey.GitHubToken in secrets.values)
    }

    @Test
    fun `restore runs once`() = runTest {
        val secrets = InMemorySecretStore()
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        secrets.values[SecretKey.GitHubToken] = goodToken
        repo.restore()
        assertEquals(GitHubAuth.SignedOut(), ready(repo).github)
    }

    @Test
    fun `signing in verifies with GitHub, then persists the token`() = runTest {
        val secrets = InMemorySecretStore()
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        val viewer = repo.signInWithToken("  $goodToken  ").orFail()
        assertEquals("RhizoNymph", viewer.login)
        assertEquals(goodToken, secrets.values[SecretKey.GitHubToken])
        assertEquals(GitHubAuth.SignedIn, ready(repo).github)
    }

    @Test
    fun `a rejected token is not saved and is cleared from the backend`() = runTest {
        val backend = testBackend(signedIn = false, paired = false)
        val secrets = InMemorySecretStore()
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        val result = repo.signInWithToken("not-a-token")
        assertInstanceOf(BackendError.GitHubAuthFailed::class.java, (result as Outcome.Err).error)
        assertTrue(secrets.values.isEmpty())
        assertEquals(GitHubStatus.NoToken, backend.githubStatus())
        assertInstanceOf(GitHubAuth.SignedOut::class.java, ready(repo).github)
    }

    @Test
    fun `a blank token is refused without asking GitHub`() = runTest {
        val backend = testBackend(signedIn = false, paired = false)
        backend.failNext(FakeCall.SetGitHubToken, BackendError.Internal("should not be called"))
        val repo = SessionRepository(backend, InMemorySecretStore(), "Pixel")
        repo.restore()
        assertInstanceOf(BackendError.InvalidInput::class.java, (repo.signInWithToken("  ") as Outcome.Err).error)
    }

    @Test
    fun `a token that cannot be saved fails the sign-in`() = runTest {
        val secrets = InMemorySecretStore()
        secrets.failWrites[SecretKey.GitHubToken] = SecretStoreError.KeystoreUnavailable("no keystore")
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        val result = repo.signInWithToken(goodToken)
        assertInstanceOf(BackendError.Storage::class.java, (result as Outcome.Err).error)
        assertInstanceOf(GitHubAuth.SignedOut::class.java, ready(repo).github)
    }

    @Test
    fun `pairing by link stores the pairing and adopts the handed-over token when signed out`() = runTest {
        val secrets = InMemorySecretStore()
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel 9")
        repo.restore()
        val result = repo.pairWithLink("rostrum://pair?name=nymph-desk&code=WDJB-MJHT").orFail()
        assertEquals("nymph-desk", result.machine.name)
        assertEquals(result.deviceToken, secrets.values[SecretKey.DeviceToken])
        assertEquals(result.endpoint, secrets.values[SecretKey.DesktopEndpoint])
        assertEquals(result.github!!.token, secrets.values[SecretKey.GitHubToken])
        assertEquals(GitHubAuth.SignedIn, ready(repo).github)
        assertInstanceOf(DesktopLink.Paired::class.java, ready(repo).desktop)
    }

    @Test
    fun `a handed-over token for another host is refused with a notice`() = runTest {
        val secrets = InMemorySecretStore()
        val backend = FakeRostrumBackend(signedIn = false, paired = false, desktopGitHubHost = "ghe.example.com")
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        repo.pairWithLink("rostrum://pair?code=WDJB-MJHT").orFail()
        val github = ready(repo).github as GitHubAuth.SignedOut
        assertTrue(github.notice!!.contains("github.com only"))
        assertNull(secrets.values[SecretKey.GitHubToken])
        assertInstanceOf(DesktopLink.Paired::class.java, ready(repo).desktop)
    }

    @Test
    fun `pairing while signed in keeps the existing token`() = runTest {
        val secrets = InMemorySecretStore(mapOf(SecretKey.GitHubToken to goodToken))
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        repo.pairWithLink("rostrum://pair?code=WDJB-MJHT").orFail()
        assertEquals(goodToken, secrets.values[SecretKey.GitHubToken])
    }

    @Test
    fun `a failed pairing stores nothing`() = runTest {
        val secrets = InMemorySecretStore()
        val repo = SessionRepository(testBackend(signedIn = false, paired = false), secrets, "Pixel")
        repo.restore()
        val result = repo.pairManual("10.0.0.5", 8485, "sha256:wrong", "WDJB-MJHT")
        assertInstanceOf(BackendError.CertificateMismatch::class.java, (result as Outcome.Err).error)
        assertTrue(secrets.values.isEmpty())
        assertEquals(DesktopLink.NotPaired, ready(repo).desktop)
    }

    @Test
    fun `signing out forgets the token everywhere`() = runTest {
        val backend = testBackend(signedIn = false, paired = false)
        val secrets = InMemorySecretStore(mapOf(SecretKey.GitHubToken to goodToken))
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        repo.signOut()
        assertEquals(GitHubAuth.SignedOut(null), ready(repo).github)
        assertNull(secrets.values[SecretKey.GitHubToken])
        assertEquals(GitHubStatus.NoToken, backend.githubStatus())
    }

    @Test
    fun `unpairing forgets the desktop, even if it already forgot this phone`() = runTest {
        val backend = testBackend(signedIn = true, paired = true)
        val secrets = InMemorySecretStore(mapOf(SecretKey.DesktopEndpoint to "{}", SecretKey.DeviceToken to "rdt_1"))
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        backend.failNext(FakeCall.Unpair, BackendError.DeviceRevoked)
        repo.unpair().orFail()
        assertEquals(DesktopLink.NotPaired, ready(repo).desktop)
        assertTrue(secrets.values.isEmpty())
    }

    @Test
    fun `an unreachable desktop keeps the pairing on unpair`() = runTest {
        val backend = testBackend(signedIn = true, paired = true)
        val secrets = InMemorySecretStore(mapOf(SecretKey.DesktopEndpoint to "{}", SecretKey.DeviceToken to "rdt_1"))
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        backend.failNext(FakeCall.Unpair, BackendError.DesktopUnreachable("timeout"))
        assertInstanceOf(Outcome.Err::class.java, repo.unpair())
        assertEquals("rdt_1", secrets.values[SecretKey.DeviceToken])
    }

    @Test
    fun `a token refreshed from the desktop is saved and signs in`() = runTest {
        val backend = FakeRostrumBackend(signedIn = false, paired = true)
        val secrets = InMemorySecretStore()
        val repo = SessionRepository(backend, secrets, "Pixel")
        repo.restore()
        repo.refreshTokenFromDesktop().orFail()
        assertTrue(secrets.values[SecretKey.GitHubToken]!!.startsWith("gho_"))
        assertInstanceOf(GitHubAuth.SignedIn::class.java, ready(repo).github)
    }
}
