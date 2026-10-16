package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.model.BudgetLevel
import io.github.rhizonymph.rostrum.data.model.CiGridFilter
import io.github.rhizonymph.rostrum.data.model.CiTrigger
import io.github.rhizonymph.rostrum.data.model.PauseReason
import io.github.rhizonymph.rostrum.data.model.PollPace
import io.github.rhizonymph.rostrum.data.model.PollSchedule
import io.github.rhizonymph.rostrum.data.model.RateWindow
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.testing.TEST_NOW
import io.github.rhizonymph.rostrum.testing.orFail
import io.github.rhizonymph.rostrum.testing.testBackend
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class FakeBudgetTest {
    private val until = TEST_NOW.plusSeconds(600)

    @Test
    fun `the sample budget is on every snapshot and nothing when signed out`() = runTest {
        val backend = testBackend()
        val budget = backend.cachedFeed().orFail().budget
        assertNotNull(budget)
        assertEquals(4123, budget?.graphql?.remaining)
        assertEquals(PollSchedule(60, 60, PollPace.Normal), backend.pollSchedule().orFail())
        val signedOut = testBackend(signedIn = false)
        assertNull(signedOut.apiBudget().orFail())
        assertNull(signedOut.cachedFeed().orFail().budget)
    }

    @Test
    fun `a low or critical budget stretches the schedule like the core's floors`() = runTest {
        val backend = testBackend()
        backend.budget.graphql = RateWindow(800, 5000, until, BudgetLevel.Low)
        assertEquals(PollSchedule(120, 60, PollPace.Stretched(BudgetLevel.Low)), backend.pollSchedule().orFail())
        backend.budget.graphql = RateWindow(100, 5000, until, BudgetLevel.Critical)
        assertEquals(240L, backend.pollSchedule().orFail().delaySecs)
    }

    @Test
    fun `while paused nothing is fetched and explicit calls say until when`() = runTest {
        val backend = testBackend()
        backend.budget.pausedUntil = until
        backend.budget.pauseReason = PauseReason.PrimaryLimit
        assertEquals(PollSchedule(600, 60, PollPace.Paused(until, PauseReason.PrimaryLimit)), backend.pollSchedule().orFail())
        assertEquals(until, backend.apiBudget().orFail()?.pausedUntil)

        assertEquals(Outcome.Err(BackendError.GitHubRateLimited(until)), backend.refreshFeed())
        assertEquals(Outcome.Err(BackendError.GitHubRateLimited(until)), backend.checkNotifications())
        val polled = backend.pollFeed().orFail()
        assertEquals(until, polled.budget?.pausedUntil)
        assertEquals(0, backend.feedRefreshes)

        val filter = CiGridFilter()
        backend.refreshCi(filter, CiTrigger.Opened).orFail()
        backend.refreshCi(filter, CiTrigger.Watch).orFail()
        assertEquals(Outcome.Err(BackendError.GitHubRateLimited(until)), backend.refreshCi(filter))
        assertEquals(0, backend.ci.fetches)
        assertEquals(listOf(CiTrigger.Opened, CiTrigger.Watch, CiTrigger.Explicit), backend.ci.triggers)
    }

    @Test
    fun `a poll refreshes every repository when nothing is paused`() = runTest {
        val backend = testBackend()
        val polled = backend.pollFeed().orFail()
        assertEquals(1, backend.feedPolls)
        assertTrue(polled.repos.all { it.load == RepoLoad.Loaded(TEST_NOW) })
    }

    @Test
    fun `the issue cadence is a clamped setting`() = runTest {
        val backend = testBackend()
        assertEquals(300L, backend.settings().orFail().issueRefreshSecs)
        assertEquals(10L, backend.setIssueRefreshInterval(1).orFail().issueRefreshSecs)
        assertEquals(86_400L, backend.setIssueRefreshInterval(1_000_000).orFail().issueRefreshSecs)
    }
}
