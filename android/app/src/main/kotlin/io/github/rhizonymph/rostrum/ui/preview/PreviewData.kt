package io.github.rhizonymph.rostrum.ui.preview

import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.fake.FakeRostrumBackend
import kotlinx.coroutines.runBlocking
import java.time.Clock

/**
 * Sample data for `@Preview`s, taken from the fake backend so previews show
 * the same pull requests the app runs on. Previews only: this blocks.
 */
object PreviewData {
    val clock: Clock = Clock.systemDefaultZone()

    fun backend(): FakeRostrumBackend = FakeRostrumBackend(clock = clock, signedIn = true, paired = true)

    /** Run one backend call synchronously and return its value (previews only). */
    fun <T> sample(call: suspend FakeRostrumBackend.() -> Outcome<T>): T = runBlocking {
        when (val outcome = backend().call()) {
            is Outcome.Ok -> outcome.value
            is Outcome.Err -> error("preview sample failed: ${outcome.error}")
        }
    }
}
