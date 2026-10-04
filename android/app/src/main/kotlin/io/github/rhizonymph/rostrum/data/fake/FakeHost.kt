package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.LabelView
import io.github.rhizonymph.rostrum.data.model.PrSummary
import java.time.Clock
import java.time.Instant

/**
 * What the fake's delegated parts ([FakeIssuesApi], [FakeStackActions]) need
 * from the [FakeRostrumBackend] that owns them: its call plumbing (latency,
 * injected failures, one lock), sign-in, the desktop, and the feed.
 */
internal interface FakeHost {
    suspend fun <T> call(op: FakeCall, block: () -> Outcome<T>): Outcome<T>

    fun <T> signedIn(block: () -> Outcome<T>): Outcome<T>

    val viewerLogin: String?

    val isPaired: Boolean

    fun labelsOf(repo: String): Map<String, LabelView>

    /** Open pull requests of [repo], as the feed shows them. */
    fun openPulls(repo: String): List<PrSummary>

    fun highestPullNumber(repo: String): Int

    fun emitFeed()
}

/** The owner's [FakeHost], bound once the owner is built (the parts are built first, for delegation). */
internal class FakeHostRef : FakeHost {
    lateinit var host: FakeHost

    override suspend fun <T> call(op: FakeCall, block: () -> Outcome<T>): Outcome<T> = host.call(op, block)
    override fun <T> signedIn(block: () -> Outcome<T>): Outcome<T> = host.signedIn(block)
    override val viewerLogin: String? get() = host.viewerLogin
    override val isPaired: Boolean get() = host.isPaired
    override fun labelsOf(repo: String): Map<String, LabelView> = host.labelsOf(repo)
    override fun openPulls(repo: String): List<PrSummary> = host.openPulls(repo)
    override fun highestPullNumber(repo: String): Int = host.highestPullNumber(repo)
    override fun emitFeed() = host.emitFeed()
}

/** The parts [FakeRostrumBackend] delegates [io.github.rhizonymph.rostrum.data.IssuesApi] and the stack actions to. */
internal class FakeParts(clock: Clock) {
    val started: Instant = clock.instant()
    val host = FakeHostRef()
    val issues = FakeIssues(
        clock = clock,
        started = started,
        labelsOf = { host.labelsOf(it) },
        highestPullNumber = { host.highestPullNumber(it) },
        onChanged = { host.emitFeed() },
    )
    val issuesApi = FakeIssuesApi(host, issues)
    val stacks = FakeStackActions(host, clock)
}
