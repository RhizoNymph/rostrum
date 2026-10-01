package io.github.rhizonymph.rostrum.ui.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertSame
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class GraphViewModelStoresTest {
    private class Probe : ViewModel() {
        var cleared = false

        override fun onCleared() {
            cleared = true
        }
    }

    private fun probeIn(owner: androidx.lifecycle.ViewModelStoreOwner): Probe =
        ViewModelProvider.create(owner, viewModelFactory { initializer { Probe() } })[Probe::class]

    private val desk = GraphKey("p1", signedIn = true)
    private val work = GraphKey("p2", signedIn = true)

    @Test
    fun `the same graph gets the same store back, as after rotation`() {
        val stores = GraphViewModelStores()
        val owner = stores.ownerFor(desk)
        val probe = probeIn(owner)
        assertSame(owner, stores.ownerFor(desk))
        assertSame(probe, probeIn(stores.ownerFor(desk)))
        assertFalse(probe.cleared)
    }

    @Test
    fun `another graph clears the previous one's ViewModels`() {
        val stores = GraphViewModelStores()
        val probe = probeIn(stores.ownerFor(desk))
        stores.ownerFor(work)
        assertTrue(probe.cleared)
    }

    @Test
    fun `signing out of the same profile is another graph too`() {
        val stores = GraphViewModelStores()
        val probe = probeIn(stores.ownerFor(desk))
        stores.ownerFor(desk.copy(signedIn = false))
        assertTrue(probe.cleared)
    }
}
