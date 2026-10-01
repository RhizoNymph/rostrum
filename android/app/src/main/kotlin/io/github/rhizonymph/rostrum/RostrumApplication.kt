package io.github.rhizonymph.rostrum

import android.app.Application
import io.github.rhizonymph.rostrum.di.AppContainer

/** Owns the [AppContainer] for the life of the process. */
class RostrumApplication : Application() {
    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        container = AppContainer(this)
        container.start()
    }
}
