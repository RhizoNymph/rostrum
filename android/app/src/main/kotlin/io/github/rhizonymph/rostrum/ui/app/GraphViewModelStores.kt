package io.github.rhizonymph.rostrum.ui.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner

/** Which navigation graph is on screen: a profile's (signed in or not), or the first-run one. */
data class GraphKey(val profile: String?, val signedIn: Boolean)

/**
 * The ViewModels of the graph on screen, in a store of their own. Asking for
 * a different [GraphKey] (a profile switch, signing in or out) clears the
 * previous graph's store, so nothing built over the old profile's backend
 * keeps running; asking again for the same key (after rotation) returns the
 * same store. Lives in the activity's store.
 */
class GraphViewModelStores : ViewModel() {
    private var current: Pair<GraphKey, ViewModelStoreOwner>? = null

    fun ownerFor(key: GraphKey): ViewModelStoreOwner {
        current?.let { (shown, owner) ->
            if (shown == key) return owner
            owner.viewModelStore.clear()
        }
        val store = ViewModelStore()
        val owner = object : ViewModelStoreOwner {
            override val viewModelStore: ViewModelStore = store
        }
        current = key to owner
        return owner
    }

    override fun onCleared() {
        current?.second?.viewModelStore?.clear()
        current = null
    }
}
