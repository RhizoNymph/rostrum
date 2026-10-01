package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.GitHubStatus
import io.github.rhizonymph.rostrum.data.model.SortDirection
import io.github.rhizonymph.rostrum.data.model.RepoSortKey
import io.github.rhizonymph.rostrum.data.model.ItemSortKey
import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.FeedTab
import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.testing.orFail
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.AfterAll
import org.junit.jupiter.api.BeforeAll
import org.junit.jupiter.api.MethodOrderer
import org.junit.jupiter.api.Order
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.TestInstance
import org.junit.jupiter.api.TestMethodOrder
import uniffi.rostrum_ffi.ProfileRegistry
import java.io.File
import java.nio.file.Files
import java.util.Base64

/**
 * The real core, through the real generated bindings, on the JVM: one core
 * over a temp directory, no network. Run with `./gradlew :app:hostSmokeTest`,
 * which builds the host `librostrum_ffi.so` and puts it on `jna.library.path`;
 * skipped anywhere else.
 */
@Tag("host-smoke")
@TestInstance(TestInstance.Lifecycle.PER_CLASS)
@TestMethodOrder(MethodOrderer.OrderAnnotation::class)
class HostSmokeTest {
    /** One core for the class, as in the app (one per process). */
    private lateinit var dir: File
    private lateinit var backend: FfiRostrumBackend

    @BeforeAll
    fun open() {
        assumeTrue(System.getProperty("rostrum.hostTests") == "true", "the host library is not built; run :app:hostSmokeTest")
        dir = Files.createTempDirectory("rostrum-host-smoke").toFile()
        backend = FfiRostrumBackend(File(dir, "core"))
    }

    @AfterAll
    fun cleanUp() {
        if (::dir.isInitialized) dir.deleteRecursively()
    }

    private fun <T> Outcome<T>.error(): BackendError = (this as Outcome.Err).error

    @Test
    @Order(1)
    fun `the core opens with no token and no warnings`(): Unit = runBlocking {
        assertEquals(GitHubStatus.NoToken, backend.githubStatus())
        assertEquals(emptyList<String>(), backend.warnings())
    }

    @Test
    @Order(2)
    fun `settings come back in range`(): Unit = runBlocking {
        val settings = backend.settings().orFail()
        assertTrue(settings.refreshIntervalSecs in 10..3600, "interval ${settings.refreshIntervalSecs}")
        assertTrue(settings.prsPerRepo in 1..100, "prs per repo ${settings.prsPerRepo}")
        assertEquals(60, backend.setRefreshInterval(60).orFail().refreshIntervalSecs.toInt())
    }

    @Test
    @Order(3)
    fun `adding a repository validates it`(): Unit = runBlocking {
        assertInstanceOf(BackendError.InvalidRepo::class.java, backend.addRepo("not a repository").error())
        assertEquals("serde-rs/serde", backend.addRepo("https://github.com/serde-rs/serde").orFail())
        assertEquals(BackendError.DuplicateRepo("serde-rs/serde"), backend.addRepo("serde-rs/serde").error())
        assertTrue("serde-rs/serde" in backend.settings().orFail().repos)
    }

    @Test
    @Order(4)
    fun `the cached feed paints without a token or network`(): Unit = runBlocking {
        val feed = backend.cachedFeed().orFail()
        assertTrue(feed.repos.any { it.repo == "serde-rs/serde" }, "sections ${feed.repos.map { it.repo }}")
        assertEquals(0, feed.totalOpen)
    }

    @Test
    @Order(5)
    fun `feed changes reach feedUpdates through the observer`(): Unit = runBlocking {
        val before = backend.cachedFeed().orFail().revision
        val next = async { withTimeout(10_000) { backend.feedUpdates.first { it.revision > before } } }
        val toggled = backend.toggleCollapsed("serde-rs/serde").orFail()
        val observed = next.await()
        assertTrue(observed.revision >= toggled.revision - 1, "observed ${observed.revision}, call ${toggled.revision}")
    }

    @Test
    @Order(6)
    fun `a pairing link is read without contacting anything`(): Unit = runBlocking {
        val fingerprint = byteArrayOf(0x4F, 0x2A, 0x91.toByte(), 0xC0.toByte(), 0x7E, 0x3B) + ByteArray(26)
        val fp = Base64.getUrlEncoder().withoutPadding().encodeToString(fingerprint)
        val link = "rostrum://pair?v=1&m=nymph-desk&h=192.168.1.24,nymph-desk.local&p=8485&c=WDJB-MJHT&fp=$fp"
        val preview = backend.parsePairingLink(link).orFail()
        assertEquals("nymph-desk", preview.machine)
        assertEquals(listOf("192.168.1.24", "nymph-desk.local"), preview.hosts)
        assertEquals(8485, preview.port)
        assertEquals("4F2A · 91C0 · 7E3B", preview.fingerprintShort)
        assertEquals("WDJBMJHT", preview.code.filter { it.isLetterOrDigit() })
        assertInstanceOf(BackendError.InvalidInput::class.java, backend.parsePairingLink("https://example.com/pair").error())
    }

    @Test
    @Order(7)
    fun `markdown renders as the timeline shows it`() {
        val blocks = backend.renderMarkdown("Some **bold** and `code`\n\n- an item", "rust-lang/rust").orFail()
        assertEquals(MdBlockKind.Paragraph, blocks.first().kind)
        assertTrue(blocks.first().spans.any { it.bold && it.text == "bold" })
        assertTrue(blocks.first().spans.any { it.code && it.text == "code" })
        assertInstanceOf(MdBlockKind.ListItem::class.java, blocks.last().kind)
    }

    @Test
    @Order(8)
    fun `core errors arrive as typed BackendErrors`(): Unit = runBlocking {
        assertEquals(BackendError.NotSignedIn, backend.refreshFeed().error())
        assertEquals(BackendError.NotPaired, backend.machineInfo().error())
        assertEquals(BackendError.NotPaired, backend.desktopConfig().error())
        assertEquals(BackendError.NotPaired, backend.copyDesktopConfig().error())
        assertInstanceOf(BackendError::class.java, backend.pullHeader(PrRef("a/b", 1)).error())
        assertInstanceOf(BackendError.InvalidInput::class.java, backend.fileDiff(PrRef("a/b", 1), -1).error())
    }

    @Test
    @Order(9)
    fun `a token is held in memory only`(): Unit = runBlocking {
        val token = "ghp_hostSmokeTestTokenThatMustNeverBeWritten0"
        assertEquals(GitHubStatus.Unverified, backend.setGitHubToken(token).orFail())
        backend.addRepo("tokio-rs/tokio").orFail()
        val leaked = dir.walkTopDown().filter { it.isFile }.filter { file ->
            file.readBytes().toString(Charsets.ISO_8859_1).contains(token)
        }.toList()
        assertEquals(emptyList<File>(), leaked)
        assertEquals(GitHubStatus.NoToken, backend.setGitHubToken(null).orFail())
        assertFalse(backend.githubStatus() is GitHubStatus.Verified)
    }

    @Test
    @Order(10)
    fun `the profile registry keeps one core per profile and removes it`(): Unit = runBlocking {
        val root = File(dir, "profiles")
        // The generated registry directly: one cached core per id. Each call
        // hands Kotlin a new wrapper, so "the same" shows as shared state.
        val registry = ProfileRegistry.open(File(dir, "raw-profiles").absolutePath)
        val raw = registry.createTokenProfile("raw")
        registry.core(raw.id).addRepo("serde-rs/serde")
        assertTrue(registry.core(raw.id).settings().repos.contains("serde-rs/serde"))
        registry.removeProfile(raw.id)
        assertTrue(registry.profiles().isEmpty())

        // Through the adapter, as the app uses it.
        val profiles = FfiProfileRegistry(root)
        assertEquals(emptyList<Any>(), profiles.profiles().orFail())
        assertEquals(null, profiles.activeProfile().orFail())
        val work = profiles.createTokenProfile("Work").orFail()
        assertEquals(ProfileKind.TokenOnly, work.kind)
        assertEquals(16, work.id.value.length)
        assertInstanceOf(BackendError.InvalidInput::class.java, profiles.createTokenProfile("  ").error())
        assertEquals(work.id, profiles.setActiveProfile(work.id).orFail().id)
        assertEquals(work.id, profiles.activeProfile().orFail())
        assertEquals("Home", profiles.renameProfile(work.id, "Home").orFail().label)
        assertEquals("octocat", profiles.setProfileLogin(work.id, "octocat").orFail().githubLogin)

        // Its backend is a real core in the profile's own directory.
        val backend = profiles.backend(work.id)
        assertEquals(GitHubStatus.NoToken, backend.githubStatus())
        backend.addRepo("tokio-rs/tokio").orFail()
        assertTrue(File(root, "profiles/${work.id.value}").isDirectory)

        val preview = profiles.parsePairingLink(
            "rostrum://pair?v=1&m=deskb&h=10.0.0.2&p=9485&c=WDJB-MJHT&fp=" +
                Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32) { 7 }),
        ).orFail()
        assertEquals("deskb", preview.machine)
        assertEquals(1, profiles.profiles().orFail().size, "reading a link makes no profile")

        profiles.removeProfile(work.id).orFail()
        assertEquals(emptyList<Any>(), profiles.profiles().orFail())
        assertEquals(null, profiles.activeProfile().orFail())
        assertFalse(File(root, "profiles/${work.id.value}").exists())
        val gone = ProfileId.of("0123456789abcdef")!!
        assertEquals(BackendError.ProfileNotFound("0123456789abcdef"), profiles.setActiveProfile(gone).error())
    }

    @Test
    @Order(11)
    fun `tabs, sorts and the repository screen answer without a token`(): Unit = runBlocking {
        val settings = backend.sortSettings().orFail()
        assertEquals(RepoSortKey.entries.size, settings.repoOptions.size)
        assertEquals(ItemSortKey.entries.size, settings.itemOptions.size)
        val byTitle = backend.setItemSort(ItemSortKey.Title, null).orFail()
        assertEquals(ItemSortKey.Title, byTitle.sort.itemKey)
        assertEquals(SortDirection.Ascending, byTitle.sort.itemDirection)
        val reversed = backend.setItemSort(ItemSortKey.Title, SortDirection.Descending).orFail()
        assertEquals(SortDirection.Descending, reversed.sort.itemDirection)
        assertEquals(ItemSortKey.Title, backend.clearFilter().orFail().sort.itemKey)

        val issuesTab = backend.setFeedTab(FeedTab.Issues).orFail()
        assertEquals(FeedTab.Issues, issuesTab.tab)
        assertEquals(FeedTab.Issues, backend.cachedFeed().orFail().tab)
        backend.setFeedTab(FeedTab.PullRequests).orFail()

        val repo = backend.settings().orFail().repos.first()
        val overview = backend.repoOverview(repo).orFail()
        assertEquals(repo, overview.repo)
        assertTrue(overview.pulls.isEmpty() && overview.issues.isEmpty())
        assertTrue(backend.trunks(repo).orFail().detected)
        assertInstanceOf(BackendError.InvalidInput::class.java, backend.setTrunks(repo, listOf("bad name")).error())
        assertInstanceOf(BackendError.InvalidInput::class.java, backend.createIssue(repo, "  ", "", emptyList(), emptyList()).error())
        assertEquals(BackendError.NotSignedIn, backend.issueDetail(IssueRef(repo, 1)).error())
    }
}
