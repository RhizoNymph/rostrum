package io.github.rhizonymph.rostrum.ui.app

import io.github.rhizonymph.rostrum.data.model.RemoteStatus
import io.github.rhizonymph.rostrum.data.session.DesktopLink
import io.github.rhizonymph.rostrum.data.session.GitHubAuth
import io.github.rhizonymph.rostrum.data.session.SessionState
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

class ShellViewModelTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val paired = SessionState.Ready(
        GitHubAuth.SignedIn,
        DesktopLink.Paired(RemoteStatus.Paired(listOf("h"), 8485, "4F2A · 91C0 · 7E3B", "h")),
    )

    @Test
    fun `the desktop badge counts waiting handoffs while paired`() = runTest(main.dispatcher) {
        val session = MutableStateFlow<SessionState>(paired)
        val vm = ShellViewModel(testBackend(), session)
        backgroundScope.launch { vm.desktopBadge.collect {} }
        runCurrent()
        assertEquals(1, vm.desktopBadge.value)
        session.value = paired.copy(desktop = DesktopLink.NotPaired)
        runCurrent()
        assertEquals(0, vm.desktopBadge.value)
    }
}
