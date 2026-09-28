package io.github.rhizonymph.rostrum.data.profiles

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.SampleDesktop
import io.github.rhizonymph.rostrum.data.model.GitHubStatus
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.secrets.SecretKey
import io.github.rhizonymph.rostrum.data.secrets.SecretStoreError
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.data.session.isSignedIn
import io.github.rhizonymph.rostrum.testing.FakeProfileRegistry
import io.github.rhizonymph.rostrum.testing.InMemorySecretVault
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.pid
import io.github.rhizonymph.rostrum.testing.testProfileManager
import io.github.rhizonymph.rostrum.ui.settings.TEST_TOKEN
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertNotEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test

private const val NYMPH_LINK =
    "rostrum://pair?name=nymph-desk&hosts=192.168.1.24&port=8485&fp=4f2a91c07e3b&code=WDJB-MJHT"
private const val FRAMEWORK_LINK =
    "rostrum://pair?name=framework&hosts=192.168.1.30&port=8485&fp=aaaabbbbcccc&code=WDJB-MJHT"
private const val NYMPH_FINGERPRINT = "4F2A · 91C0 · 7E3B"

private val DESKTOP_SECRETS = mapOf(
    SecretKey.GitHubToken to TEST_TOKEN,
    SecretKey.DesktopEndpoint to """{"hosts":["192.168.1.24"],"port":8485}""",
    SecretKey.DeviceToken to "rdt_test",
)

class ProfileManagerTest {
    private val registry = FakeProfileRegistry()
    private val vault = InMemorySecretVault()

    private fun TestScope.manager(legacy: LegacyCleanup = LegacyCleanup.None) =
        testProfileManager(registry, vault, legacy)

    private fun ProfileManager.ready() = state.value as ProfilesState.Ready

    private fun ProfileManager.session(id: io.github.rhizonymph.rostrum.data.model.ProfileId) =
        handle(id).session.state.value as SessionState.Ready

    @Nested
    inner class Start {
        @Test
        fun `wipes the legacy state before opening the registry`() = runTest {
            var callsAtWipe: List<String>? = null
            val manager = manager {
                callsAtWipe = registry.calls.toList()
                true
            }
            manager.start()
            assertEquals(emptyList<String>(), callsAtWipe)
            assertInstanceOf(ProfilesState.Ready::class.java, manager.state.value)
            assertTrue(manager.upgradedFromSingleProfile.value)
        }

        @Test
        fun `a start with nothing legacy to wipe is not an upgrade`() = runTest {
            val manager = manager()
            manager.start()
            assertFalse(manager.upgradedFromSingleProfile.value)
        }

        @Test
        fun `with no profiles it is ready with none active`() = runTest {
            val manager = manager()
            assertEquals(ProfilesState.Starting, manager.state.value)
            manager.start()
            assertEquals(ProfilesState.Ready(emptyList(), null), manager.state.value)
        }

        @Test
        fun `restores every profile's secrets into its own core, the active one first`() = runTest {
            val first = registry.seed("nymph-desk")
            val second = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            vault.seed(first, DESKTOP_SECRETS)
            vault.seed(second, mapOf(SecretKey.GitHubToken to TEST_TOKEN))
            registry.active = first
            val manager = manager()
            manager.start()
            assertEquals(first, vault.readOrder.first())
            assertTrue(second in vault.readOrder)
            assertTrue(manager.session(first).isSignedIn)
            assertInstanceOf(DesktopLink.Paired::class.java, manager.session(first).desktop)
            assertTrue(manager.session(second).isSignedIn)
            assertEquals(DesktopLink.NotPaired, manager.session(second).desktop)
            assertNotEquals(GitHubStatus.NoToken, registry.backend(second).githubStatus())
        }

        @Test
        fun `without an active profile the most recently used becomes active`() = runTest {
            registry.seed("nymph-desk")
            val newer = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            val manager = manager()
            manager.start()
            assertEquals(newer, manager.ready().active)
            assertEquals(newer, registry.active)
        }

        @Test
        fun `records each signed-in profile's GitHub login from the viewer`() = runTest {
            val desk = registry.seed("nymph-desk")
            val signedOut = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            vault.seed(desk, DESKTOP_SECRETS)
            val manager = manager()
            manager.start()
            advanceUntilIdle()
            assertEquals("RhizoNymph", manager.ready().profile(desk)!!.githubLogin)
            assertNull(manager.ready().profile(signedOut)!!.githubLogin)
            assertEquals(setOf(desk), manager.signedInProfiles.value)
        }

        @Test
        fun `an unreadable registry is unavailable, and starting again recovers`() = runTest {
            registry.seed("nymph-desk")
            registry.failNext("profiles", BackendError.Storage("disk"))
            val manager = manager()
            manager.start()
            assertEquals(ProfilesState.Unavailable(BackendError.Storage("disk")), manager.state.value)
            manager.start()
            assertEquals(1, manager.ready().profiles.size)
        }

        @Test
        fun `starting twice restores once`() = runTest {
            vault.seed(registry.seed("nymph-desk"), DESKTOP_SECRETS)
            val manager = manager()
            manager.start()
            val reads = vault.readOrder.size
            manager.start()
            assertEquals(reads, vault.readOrder.size)
        }
    }

    @Nested
    inner class Switching {
        @Test
        fun `switching makes the profile active and most recently used`() = runTest {
            val first = registry.seed("nymph-desk")
            val second = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            registry.active = second
            val manager = manager()
            manager.start()
            assertEquals(first, manager.switchTo(first).orFail().id)
            assertEquals(first, manager.ready().active)
            assertEquals(first, manager.ready().profiles.first().id)
        }

        @Test
        fun `switching to an unknown profile fails without changing anything`() = runTest {
            val only = registry.seed("nymph-desk")
            val manager = manager()
            manager.start()
            val result = manager.switchTo(pid("gone"))
            assertEquals(Outcome.Err(BackendError.ProfileNotFound("gone")), result)
            assertEquals(only, manager.ready().active)
        }
    }

    @Nested
    inner class Pairing {
        @Test
        fun `pairing a new desktop makes a profile without changing the active one`() = runTest {
            val desk = registry.seed("nymph-desk")
            vault.seed(desk, DESKTOP_SECRETS)
            val manager = manager()
            manager.start()
            val paired = manager.pairWithLink(FRAMEWORK_LINK).orFail()
            assertTrue(paired.created)
            assertEquals("framework", paired.machine)
            assertEquals("framework", paired.profile.label)
            assertEquals(desk, manager.ready().active)
            assertEquals(2, manager.ready().profiles.size)
            // The pairing, and the token the desktop handed over, belong to the new profile only.
            val secrets = vault.of(paired.profile.id).values
            assertTrue(secrets.getValue(SecretKey.DeviceToken).startsWith("rdt_"))
            assertTrue(secrets.getValue(SecretKey.GitHubToken).startsWith("gho_"))
            assertEquals("rdt_test", vault.of(desk).values[SecretKey.DeviceToken])
            assertTrue(manager.session(paired.profile.id).isSignedIn)
        }

        @Test
        fun `pairing a known desktop again re-pairs its profile`() = runTest {
            val desk = registry.seed("nymph-desk", ProfileKind.Desktop("nymph-desk", NYMPH_FINGERPRINT))
            vault.seed(desk, DESKTOP_SECRETS)
            val other = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            registry.active = other
            val manager = manager()
            manager.start()
            val paired = manager.pairWithLink(NYMPH_LINK).orFail()
            assertFalse(paired.created)
            assertEquals(desk, paired.profile.id)
            assertEquals(2, manager.ready().profiles.size)
            assertEquals(other, manager.ready().active)
            assertNotEquals("rdt_test", vault.of(desk).values[SecretKey.DeviceToken])
            // Its own token stays.
            assertEquals(TEST_TOKEN, vault.of(desk).values[SecretKey.GitHubToken])
        }

        @Test
        fun `pairing by hand pins the probed desktop`() = runTest {
            val manager = manager()
            manager.start()
            val paired = manager.pairManual("192.168.1.24", 8485, SampleDesktop.FINGERPRINT, "WDJB-MJHT")
            val profile = paired.orFail().profile
            assertEquals(ProfileKind.Desktop("nymph-desk", NYMPH_FINGERPRINT), profile.kind)
            assertEquals(listOf(profile.id), manager.ready().profiles.map { it.id })
            assertNull(manager.ready().active)
        }

        @Test
        fun `a pairing whose secrets can't be saved drops the new profile`() = runTest {
            val manager = manager()
            manager.start()
            vault.of(pid("p1")).failWrites[SecretKey.DeviceToken] = SecretStoreError.KeystoreUnavailable("gone")
            val result = manager.pairWithLink(NYMPH_LINK)
            assertInstanceOf(BackendError.Storage::class.java, (result as Outcome.Err).error)
            assertEquals(listOf(pid("p1")), registry.removed)
            assertEquals(emptyList<Any>(), manager.ready().profiles)
        }

        @Test
        fun `a failed pairing makes no profile`() = runTest {
            val manager = manager()
            manager.start()
            val result = manager.pairManual("192.168.1.24", 8485, SampleDesktop.FINGERPRINT, "0000-0000")
            assertInstanceOf(BackendError.RemoteApi::class.java, (result as Outcome.Err).error)
            assertEquals(emptyList<Any>(), manager.ready().profiles)
        }
    }

    @Nested
    inner class TokenProfiles {
        @Test
        fun `a token profile is checked with GitHub and named after the login`() = runTest {
            val manager = manager()
            manager.start()
            val profile = manager.createTokenProfile("  $TEST_TOKEN ", label = null).orFail()
            assertEquals("RhizoNymph", profile.label)
            assertEquals("RhizoNymph", profile.githubLogin)
            assertEquals(ProfileKind.TokenOnly, profile.kind)
            assertEquals(TEST_TOKEN, vault.of(profile.id).values[SecretKey.GitHubToken])
            assertTrue(manager.session(profile.id).isSignedIn)
            // Creating is not switching.
            assertNull(manager.ready().active)
        }

        @Test
        fun `a named token profile keeps its name`() = runTest {
            val manager = manager()
            manager.start()
            assertEquals("Work", manager.createTokenProfile(TEST_TOKEN, label = " Work ").orFail().label)
        }

        @Test
        fun `a rejected token keeps nothing`() = runTest {
            val manager = manager()
            manager.start()
            val result = manager.createTokenProfile("not-a-token", label = null)
            assertInstanceOf(BackendError.GitHubAuthFailed::class.java, (result as Outcome.Err).error)
            assertEquals(listOf(pid("p1")), registry.removed)
            assertFalse(pid("p1") in vault.profiles)
            assertEquals(emptyList<Any>(), manager.ready().profiles)
        }

        @Test
        fun `a blank token makes no profile`() = runTest {
            val manager = manager()
            manager.start()
            assertInstanceOf(BackendError.InvalidInput::class.java, (manager.createTokenProfile(" ", null) as Outcome.Err).error)
            assertFalse("createTokenProfile" in registry.calls)
        }
    }

    @Nested
    inner class Removing {
        @Test
        fun `removing an inactive profile deletes its data and secrets only`() = runTest {
            val gone = registry.seed("nymph-desk")
            val kept = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            vault.seed(gone, DESKTOP_SECRETS)
            vault.seed(kept, DESKTOP_SECRETS)
            registry.active = kept
            val manager = manager()
            manager.start()
            val removal = manager.remove(gone).orFail()
            assertEquals(ProfileRemoval.Inactive::class, removal::class)
            assertEquals(listOf(gone), registry.removed)
            assertFalse(gone in vault.profiles)
            assertEquals(DESKTOP_SECRETS, vault.of(kept).values)
            assertEquals(ProfilesState.Ready(listOf(registry.profile(kept)!!), kept), manager.state.value)
        }

        @Test
        fun `removing the active profile switches to the most recently used of the rest`() = runTest {
            val oldest = registry.seed("a")
            val middle = registry.seed("b", ProfileKind.TokenOnly)
            val active = registry.seed("c", ProfileKind.TokenOnly)
            registry.active = active
            val manager = manager()
            manager.start()
            val removal = manager.remove(active).orFail() as ProfileRemoval.ActiveReplaced
            assertEquals(middle, removal.next.id)
            assertEquals(middle, manager.ready().active)
            assertEquals(listOf(middle, oldest), manager.ready().profiles.map { it.id })
        }

        @Test
        fun `removing the last profile leaves none active`() = runTest {
            val only = registry.seed("nymph-desk")
            val manager = manager()
            manager.start()
            assertInstanceOf(ProfileRemoval.LastRemoved::class.java, manager.remove(only).orFail())
            assertEquals(ProfilesState.Ready(emptyList(), null), manager.state.value)
        }

        @Test
        fun `a failed removal keeps the profile and its secrets`() = runTest {
            val only = registry.seed("nymph-desk")
            vault.seed(only, DESKTOP_SECRETS)
            val manager = manager()
            manager.start()
            registry.failNext("removeProfile", BackendError.Storage("busy"))
            assertEquals(Outcome.Err(BackendError.Storage("busy")), manager.remove(only))
            assertEquals(DESKTOP_SECRETS, vault.of(only).values)
            assertEquals(only, manager.ready().active)
        }
    }

    @Nested
    inner class Others {
        @Test
        fun `renaming trims and refuses a blank name`() = runTest {
            val id = registry.seed("nymph-desk")
            val manager = manager()
            manager.start()
            assertEquals("Home", manager.rename(id, "  Home ").orFail().label)
            assertEquals("Home", manager.ready().profile(id)!!.label)
            assertInstanceOf(BackendError.InvalidInput::class.java, (manager.rename(id, "   ") as Outcome.Err).error)
        }

        @Test
        fun `notifications are wanted while any signed-in profile has a toggle on`() = runTest {
            val signedOut = registry.seed("nymph-desk")
            val quiet = registry.seed("framework", ProfileKind.Desktop("framework", "AAAA · BBBB · CCCC"))
            vault.seed(quiet, mapOf(SecretKey.GitHubToken to TEST_TOKEN))
            val manager = manager()
            manager.start()
            registry.backend(quiet).setNotifications(newPullRequests = false, reviewRequests = false)
            // The signed-out profile's toggles are on, but it can't check anything.
            assertFalse(manager.wantsNotifications())
            registry.backend(quiet).setNotifications(newPullRequests = false, reviewRequests = true)
            assertTrue(manager.wantsNotifications())
            assertEquals(GitHubAuth.SignedOut(), manager.session(signedOut).github)
        }

        @Test
        fun `signing a profile out drops it from the signed-in set and clears its login`() = runTest {
            val id = registry.seed("nymph-desk")
            vault.seed(id, DESKTOP_SECRETS)
            val manager = manager()
            manager.start()
            advanceUntilIdle()
            manager.handle(id).session.signOut()
            advanceUntilIdle()
            assertEquals(emptySet<Any>(), manager.signedInProfiles.value)
            assertNull(manager.ready().profile(id)!!.githubLogin)
        }
    }
}
