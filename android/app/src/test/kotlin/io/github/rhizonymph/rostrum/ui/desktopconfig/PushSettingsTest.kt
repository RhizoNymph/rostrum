package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import io.github.rhizonymph.rostrum.data.model.ConfigChange
import io.github.rhizonymph.rostrum.data.model.ConfigField
import io.github.rhizonymph.rostrum.data.model.ConfigPushResult
import io.github.rhizonymph.rostrum.data.model.DesktopConfigPreview
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.Settings
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.ActionState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

/** "Send settings to <machine>": the change list, the fake's revision check, and the sheet. */
@OptIn(ExperimentalCoroutinesApi::class)
class PushSettingsTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    @Nested
    inner class ChangeText {
        @Test
        fun `a list field is shown as what it adds and removes`() {
            val change = ConfigChange(ConfigField.Repos, "Repositories", "a/one, b/two, c/three", "a/one, c/three, d/four")
            assertEquals(
                ConfigChangeView.ListDiff("Repositories", added = listOf("d/four"), removed = listOf("b/two"), reordered = false),
                ConfigChangeText.view(change),
            )
        }

        @Test
        fun `none is an empty list and a reorder alone is said as one`() {
            assertEquals(
                ConfigChangeView.ListDiff("Authors", added = listOf("ada-lin"), removed = emptyList(), reordered = false),
                ConfigChangeText.view(ConfigChange(ConfigField.Authors, "Authors", "(none)", "ada-lin")),
            )
            assertEquals(
                ConfigChangeView.ListDiff("Repositories", added = emptyList(), removed = emptyList(), reordered = true),
                ConfigChangeText.view(ConfigChange(ConfigField.Repos, "Repositories", "a/one, b/two", "b/two, a/one")),
            )
        }

        @Test
        fun `switches read as on and off, and unset as not set`() {
            assertEquals(
                ConfigChangeView.Value("Hide drafts", "off", "on"),
                ConfigChangeText.view(ConfigChange(ConfigField.HideDrafts, "Hide drafts", "false", "true")),
            )
            assertEquals(
                ConfigChangeView.Value("Issues per repository", "not set", "40"),
                ConfigChangeText.view(ConfigChange(ConfigField.IssuesPerRepo, "Issues per repository", "(unset)", "40")),
            )
            assertEquals(
                ConfigChangeView.Value("Pull requests per repository", "25", "30"),
                ConfigChangeText.view(ConfigChange(ConfigField.PrsPerRepo, "Pull requests per repository", "25", "30")),
            )
        }

        @Test
        fun `trunks are a list of per-repository entries`() {
            val change = ConfigChange(ConfigField.Trunks, "Trunks", "(none)", "a/one: main develop")
            assertEquals(
                ConfigChangeView.ListDiff("Trunks", added = listOf("a/one: main develop"), removed = emptyList(), reordered = false),
                ConfigChangeText.view(change),
            )
        }
    }

    @Nested
    inner class CopyPreviewFields {
        private val preview = DesktopConfigPreview(
            machine = "framework",
            repos = listOf("a/one"),
            added = emptyList(),
            removed = emptyList(),
            prsPerRepo = 25,
            hideDrafts = true,
            hideEmptyRepos = false,
            authors = emptyList(),
            includeInvolved = false,
            autostash = true,
            changesAnything = true,
            revision = "r1",
            issuesPerRepo = 40,
        )

        private val phone = Settings(
            repos = listOf("a/one"),
            refreshIntervalSecs = 60,
            prsPerRepo = 25,
            notifyNewPullRequests = false,
            notifyReviewRequests = false,
            autostash = true,
            feed = FeedPreferences(hideDrafts = true, hideEmptyRepos = false, authors = emptyList(), includeInvolved = false),
            issuesPerRepo = 25,
        )

        @Test
        fun `the summary names issues per repository when the desktop shares it`() {
            assertEquals(
                "25 pull requests and 40 issues per repository · drafts hidden · empty repositories shown · everyone · stash on",
                DesktopConfigText.preferencesSummary(preview),
            )
        }

        @Test
        fun `copying names issues per repository, sorts and trunks when they differ`() {
            val withSorts = preview.copy(
                copyChanges = listOf(
                    ConfigChange(ConfigField.IssuesPerRepo, "Issues per repository", "25", "40"),
                    ConfigChange(ConfigField.ItemSort, "Item order", "created desc", "updated desc"),
                    ConfigChange(ConfigField.Trunks, "Trunks", "(none)", "a/one: main"),
                ),
            )
            assertEquals(
                listOf("Changes issues per repository (25 → 40), sorts and trunks"),
                DesktopConfigText.changeLines(withSorts, phone),
            )
        }

        @Test
        fun `a desktop that does not share issues per repository leaves it out`() {
            assertEquals(emptyList<String>(), DesktopConfigText.changeLines(preview.copy(issuesPerRepo = null), phone))
        }
    }

    @Nested
    inner class FakeDesktop {
        @Test
        fun `the preview carries a revision and what a push would change`() = runTest {
            val preview = testBackend().desktopConfig().orFail()
            assertEquals("r1", preview.revision)
            assertEquals(25, preview.issuesPerRepo)
            assertEquals(
                listOf(ConfigField.Repos, ConfigField.PrsPerRepo, ConfigField.HideDrafts, ConfigField.Authors, ConfigField.IncludeInvolved, ConfigField.Autostash),
                preview.pushChanges.map { it.field },
            )
            val prs = preview.pushChanges.single { it.field == ConfigField.PrsPerRepo }
            assertEquals("25" to "30", prs.before to prs.after)
        }

        @Test
        fun `a push at the current revision is applied and leaves nothing to send`() = runTest {
            val backend = testBackend()
            val base = backend.desktopConfig().orFail().revision
            val result = backend.pushConfigToDesktop(base).orFail()
            assertInstanceOf(ConfigPushResult.Applied::class.java, result)
            assertEquals("r2", result.desktop.revision)
            assertEquals(emptyList<ConfigChange>(), result.desktop.pushChanges)
            assertFalse(backend.desktopConfig().orFail().changesAnything)
        }

        @Test
        fun `a push at an older revision writes nothing and returns the new difference`() = runTest {
            val backend = testBackend()
            val base = backend.desktopConfig().orFail().revision
            backend.changeDesktopConfigElsewhere(prsPerRepo = 50)
            val result = backend.pushConfigToDesktop(base).orFail()
            assertInstanceOf(ConfigPushResult.Changed::class.java, result)
            assertEquals("r2", result.desktop.revision)
            assertEquals(50, result.desktop.prsPerRepo)
            val prs = result.desktop.pushChanges.single { it.field == ConfigField.PrsPerRepo }
            assertEquals("50" to "30", prs.before to prs.after)
        }

        @Test
        fun `a blank revision is invalid and an unpaired push is NotPaired`() = runTest {
            assertInstanceOf(BackendError.InvalidInput::class.java, testBackend().pushConfigToDesktop(" ").errorOrNull())
            assertEquals(BackendError.NotPaired, testBackend(paired = false).pushConfigToDesktop("r1").errorOrNull())
        }

        @Test
        fun `copying brings issues per repository too`() = runTest {
            val backend = testBackend()
            backend.setIssuesPerRepo(60).orFail()
            assertEquals(60, backend.settings().orFail().issuesPerRepo)
            assertEquals(25, backend.copyDesktopConfig().orFail().issuesPerRepo)
        }

        @Test
        fun `issues per repository is clamped`() = runTest {
            val backend = testBackend()
            assertEquals(100, backend.setIssuesPerRepo(500).orFail().issuesPerRepo)
            assertEquals(1, backend.setIssuesPerRepo(0).orFail().issuesPerRepo)
        }
    }

    @Nested
    inner class Sheet {
        private inner class Harness(val backend: FakeRostrumBackend, val vm: PushSettingsViewModel, val messages: MutableList<String>)

        private fun TestScope.harness(backend: FakeRostrumBackend = testBackend()): Harness {
            val vm = PushSettingsViewModel(backend)
            val messages = mutableListOf<String>()
            backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.messages.flow.toList(messages) }
            return Harness(backend, vm, messages)
        }

        @Test
        fun `opening loads what sending would change`() = runTest(main.dispatcher) {
            val h = harness()
            assertEquals(PushSheetState.Closed, h.vm.state.value)
            h.vm.open()
            assertEquals(PushSheetState.Loading, h.vm.state.value)
            advanceUntilIdle()
            val ready = h.vm.state.value as PushSheetState.Ready
            assertEquals("nymph-desk", ready.preview.machine)
            assertFalse(ready.nothingToSend)
            assertFalse(ready.stale)
            assertEquals(6, ready.changes.size)
            assertEquals("Repositories", ready.changes.first().label)
        }

        @Test
        fun `cancelling sends nothing`() = runTest(main.dispatcher) {
            val h = harness()
            h.vm.open()
            advanceUntilIdle()
            h.vm.dismiss()
            advanceUntilIdle()
            assertEquals(PushSheetState.Closed, h.vm.state.value)
            assertEquals("r1", h.backend.desktopConfig().orFail().revision)
        }

        @Test
        fun `sending applies, says so and closes`() = runTest(main.dispatcher) {
            val h = harness()
            h.vm.open()
            advanceUntilIdle()
            h.vm.send()
            assertEquals(ActionState.Running, (h.vm.state.value as PushSheetState.Ready).send)
            advanceUntilIdle()
            assertEquals(PushSheetState.Closed, h.vm.state.value)
            assertEquals(listOf("Sent settings to nymph-desk"), h.messages)
            assertEquals(30, h.backend.desktopConfig().orFail().prsPerRepo)
        }

        @Test
        fun `a desktop changed since shows the fresh difference, and sending again applies`() = runTest(main.dispatcher) {
            val h = harness()
            h.vm.open()
            advanceUntilIdle()
            h.backend.changeDesktopConfigElsewhere(prsPerRepo = 50)
            h.vm.send()
            advanceUntilIdle()
            val stale = h.vm.state.value as PushSheetState.Ready
            assertTrue(stale.stale)
            assertEquals("r2", stale.preview.revision)
            assertEquals(ConfigChangeView.Value("Pull requests per repository", "50", "30"), stale.changes.single { it.label == "Pull requests per repository" })
            assertEquals(emptyList<String>(), h.messages)
            h.vm.send()
            advanceUntilIdle()
            assertEquals(PushSheetState.Closed, h.vm.state.value)
            assertEquals(30, h.backend.desktopConfig().orFail().prsPerRepo)
        }

        @Test
        fun `with nothing to send, Send does nothing`() = runTest(main.dispatcher) {
            val backend = testBackend()
            backend.pushConfigToDesktop("r1").orFail()
            val h = harness(backend)
            h.vm.open()
            advanceUntilIdle()
            val ready = h.vm.state.value as PushSheetState.Ready
            assertTrue(ready.nothingToSend)
            h.vm.send()
            advanceUntilIdle()
            assertEquals(ready, h.vm.state.value)
            assertEquals("r2", backend.desktopConfig().orFail().revision)
        }

        @Test
        fun `a failed send stays open with the error`() = runTest(main.dispatcher) {
            val h = harness()
            h.vm.open()
            advanceUntilIdle()
            h.backend.failNext(FakeCall.PushConfigToDesktop, BackendError.DesktopUnreachable("refused"))
            h.vm.send()
            advanceUntilIdle()
            val ready = h.vm.state.value as PushSheetState.Ready
            assertEquals(ActionState.Failed(BackendError.DesktopUnreachable("refused")), ready.send)
        }

        @Test
        fun `a CONFIG_CHANGED refusal reloads the preview as stale`() = runTest(main.dispatcher) {
            val h = harness()
            h.vm.open()
            advanceUntilIdle()
            h.backend.failNext(FakeCall.PushConfigToDesktop, BackendError.RemoteApi(RemoteErrorCode.ConfigChanged, "changed"))
            h.vm.send()
            advanceUntilIdle()
            val ready = h.vm.state.value as PushSheetState.Ready
            assertTrue(ready.stale)
            assertEquals(ActionState.Idle, ready.send)
        }

        @Test
        fun `a failed preview can be retried`() = runTest(main.dispatcher) {
            val h = harness()
            h.backend.failNext(FakeCall.DesktopConfig, BackendError.DesktopTimeout)
            h.vm.open()
            advanceUntilIdle()
            assertEquals(PushSheetState.Failed(BackendError.DesktopTimeout), h.vm.state.value)
            h.vm.retry()
            advanceUntilIdle()
            assertInstanceOf(PushSheetState.Ready::class.java, h.vm.state.value)
        }

        @Test
        fun `the texts name the machine`() {
            assertEquals("Send settings to framework", PushConfigText.sheetTitle("framework"))
            assertEquals("Nothing to send: framework already has this profile's settings.", PushConfigText.nothingToSend("framework"))
            assertEquals("The desktop's settings changed since you looked", PushConfigText.CHANGED_SINCE)
        }
    }
}

private fun <T> io.github.rhizonymph.rostrum.data.Outcome<T>.errorOrNull(): BackendError? =
    (this as? io.github.rhizonymph.rostrum.data.Outcome.Err)?.error
