package io.github.rhizonymph.rostrum.ui.pr

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.fake.FakeCall
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.testing.MainDispatcherExtension
import io.github.rhizonymph.rostrum.testing.TEST_CLOCK
import io.github.rhizonymph.rostrum.testing.testBackend
import io.github.rhizonymph.rostrum.ui.common.dataOrNull
import io.github.rhizonymph.rostrum.ui.components.loadEarlierText
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.extension.RegisterExtension

@OptIn(ExperimentalCoroutinesApi::class)
class PrLoadEarlierTest {
    @JvmField
    @RegisterExtension
    val main = MainDispatcherExtension()

    private val backend = testBackend()

    @Test
    fun `load earlier merges the previous page into the conversation`() = runTest(main.dispatcher) {
        val vm = PrDetailViewModel(PrRef("RhizoNymph/rostrum", 9), backend, TEST_CLOCK)
        advanceUntilIdle()
        val before = vm.state.value.detail.dataOrNull()!!
        assertTrue(before.hasEarlier)
        vm.loadEarlier()
        assertTrue(vm.state.value.loadingEarlier)
        advanceUntilIdle()
        val after = vm.state.value.detail.dataOrNull()!!
        assertFalse(after.hasEarlier)
        assertEquals(before.timeline.size + 2, after.timeline.size)
        assertFalse(vm.state.value.loadingEarlier)
    }

    @Test
    fun `a failed page keeps the conversation`() = runTest(main.dispatcher) {
        val vm = PrDetailViewModel(PrRef("RhizoNymph/rostrum", 9), backend, TEST_CLOCK)
        advanceUntilIdle()
        backend.failNext(FakeCall.LoadEarlierPull, BackendError.Network("offline"))
        vm.loadEarlier()
        advanceUntilIdle()
        assertTrue(vm.state.value.detail.dataOrNull()!!.hasEarlier)
        assertFalse(vm.state.value.loadingEarlier)
    }

    @Test
    fun `nothing earlier, nothing to load`() = runTest(main.dispatcher) {
        val vm = PrDetailViewModel(PrRef("RhizoNymph/rostrum", 10), backend, TEST_CLOCK)
        advanceUntilIdle()
        vm.loadEarlier()
        assertFalse(vm.state.value.loadingEarlier)
        assertEquals("Load earlier (3 more)", loadEarlierText(3))
        assertEquals("Load earlier", loadEarlierText(null))
    }
}
