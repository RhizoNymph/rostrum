package io.github.rhizonymph.rostrum.ui.common

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.rhizonymph.rostrum.data.profiles.ProfileHandle
import io.github.rhizonymph.rostrum.di.AppContainer

/** The app's object graph, provided at the root of the composition. */
val LocalAppContainer = staticCompositionLocalOf<AppContainer> {
    error("LocalAppContainer is not provided; wrap the UI in RostrumApp")
}

/**
 * The active profile's backend and session, provided around its navigation
 * graph; `null` in the sign-in graph shown before any profile exists. A
 * switch rebuilds the graph under the new profile's handle.
 */
val LocalProfileHandle = staticCompositionLocalOf<ProfileHandle?> { null }

/**
 * A ViewModel scoped to the current navigation destination, built from the
 * [AppContainer] by [create]. Use [key] when one destination holds several
 * instances (one per pull request, one per file).
 */
@Composable
inline fun <reified VM : ViewModel> rostrumViewModel(
    key: String? = null,
    noinline create: (AppContainer) -> VM,
): VM {
    val container = LocalAppContainer.current
    return viewModel(key = key, factory = viewModelFactory { initializer { create(container) } })
}

/**
 * As [rostrumViewModel], for screens that work on the active profile: [create]
 * also gets its [ProfileHandle]. Only valid inside a profile's graph.
 */
@Composable
inline fun <reified VM : ViewModel> profileViewModel(
    key: String? = null,
    noinline create: (AppContainer, ProfileHandle) -> VM,
): VM {
    val container = LocalAppContainer.current
    val profile = checkNotNull(LocalProfileHandle.current) { "profileViewModel outside a profile's graph" }
    return viewModel(key = key, factory = viewModelFactory { initializer { create(container, profile) } })
}
