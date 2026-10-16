package io.github.rhizonymph.rostrum.ui.feed

import io.github.rhizonymph.rostrum.data.model.ApiBudget
import io.github.rhizonymph.rostrum.data.model.BudgetLevel
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.PauseReason
import io.github.rhizonymph.rostrum.data.model.PollPace
import io.github.rhizonymph.rostrum.data.model.PollSchedule
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.model.RateWindow
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset

class BudgetChipTest {
    private val now: Instant = Instant.parse("2026-09-28T13:30:00Z")
    private val utc: ZoneId = ZoneOffset.UTC
    private val resets: Instant = Instant.parse("2026-09-28T14:05:00Z")
    private val normal = PollSchedule(60, 60, PollPace.Normal)

    private fun window(remaining: Int, level: BudgetLevel, resetsAt: Instant? = resets) =
        RateWindow(remaining, 5000, resetsAt, level)

    private fun budget(graphql: RateWindow?, schedule: PollSchedule = normal, rest: RateWindow? = null) =
        ApiBudget(graphql, rest, schedule)

    private fun paused(until: Instant, reason: PauseReason = PauseReason.SecondaryLimit) =
        PollSchedule(600, 60, PollPace.Paused(until, reason))

    @Nested
    inner class Chip {
        @Test
        fun `a healthy budget is quiet, with what is left and when it resets`() {
            val chip = budgetChip(budget(window(4123, BudgetLevel.Healthy)), now, utc)
            assertEquals(BudgetChipView("API 4123/5000 · resets 14:05", ColorRole.Neutral, "GitHub API budget, 4123 of 5000 left, resets at 14:05. Details"), chip)
        }

        @Test
        fun `low warns, critical and spent are danger`() {
            assertEquals(ColorRole.Warning, budgetChip(budget(window(999, BudgetLevel.Low)), now, utc)?.role)
            assertEquals(ColorRole.Danger, budgetChip(budget(window(499, BudgetLevel.Critical)), now, utc)?.role)
            assertEquals(ColorRole.Danger, budgetChip(budget(window(0, BudgetLevel.Exhausted)), now, utc)?.role)
        }

        @Test
        fun `times are shown in the phone's zone`() {
            val chip = budgetChip(budget(window(4123, BudgetLevel.Healthy)), now, ZoneId.of("Europe/Berlin"))
            assertEquals("API 4123/5000 · resets 16:05", chip?.text)
        }

        @Test
        fun `a window that has reset is full, with no reset time`() {
            assertEquals("API 5000/5000", budgetChip(budget(window(5000, BudgetLevel.Healthy, resetsAt = null)), now, utc)?.text)
            // A reset that passed since the snapshot was taken reads the same.
            val stale = budgetChip(budget(window(12, BudgetLevel.Critical, resetsAt = now.minusSeconds(1))), now, utc)
            assertEquals(BudgetChipView("API 5000/5000", ColorRole.Neutral, "GitHub API budget full, 5000 of 5000. Details"), stale)
        }

        @Test
        fun `a pause wins over the window and is danger`() {
            val chip = budgetChip(budget(window(4123, BudgetLevel.Healthy), paused(Instant.parse("2026-09-28T13:32:00Z"))), now, utc)
            assertEquals(BudgetChipView("paused until 13:32", ColorRole.Danger, "GitHub paused refreshing until 13:32. Details"), chip)
        }

        @Test
        fun `a pause that has run out shows the window again`() {
            val chip = budgetChip(budget(window(4123, BudgetLevel.Healthy), paused(now)), now, utc)
            assertEquals("API 4123/5000 · resets 14:05", chip?.text)
        }

        @Test
        fun `nothing shows before GitHub has said anything, or when signed out`() {
            assertNull(budgetChip(null, now, utc))
            assertNull(budgetChip(budget(null), now, utc))
            assertNull(budgetChip(budget(null, paused(now)), now, utc))
        }

        @Test
        fun `a pause shows even before any window is known`() {
            assertEquals("paused until 13:40", budgetChip(budget(null, paused(now.plusSeconds(600))), now, utc)?.text)
        }
    }

    @Nested
    inner class Explanation {
        @Test
        fun `a desktop profile's budget is shared with the desktop and gh`() {
            val sharing = BudgetSharing.of(ProfileKind.Desktop("nymph-desk", "4F2A"))
            assertEquals(BudgetSharing.Desktop("nymph-desk"), sharing)
            val lines = budgetExplanation(budget(window(4123, BudgetLevel.Healthy)), sharing, now, utc).lines
            assertTrue(lines[0].contains("handed over from nymph-desk"), lines[0])
            assertTrue(lines[0].contains("rostrum and gh there"), lines[0])
        }

        @Test
        fun `a pasted token's budget is the token's`() {
            assertEquals(BudgetSharing.PastedToken, BudgetSharing.of(ProfileKind.TokenOnly))
            assertEquals(BudgetSharing.PastedToken, BudgetSharing.of(null))
            val lines = budgetExplanation(budget(null), BudgetSharing.PastedToken, now, utc).lines
            assertTrue(lines[0].startsWith("GitHub budgets belong to the token"), lines[0])
        }

        @Test
        fun `both windows are listed, as left and reset or not yet reported`() {
            val lines = budgetExplanation(
                budget(window(4123, BudgetLevel.Healthy), rest = window(5000, BudgetLevel.Healthy, resetsAt = null)),
                BudgetSharing.PastedToken,
                now,
                utc,
            ).lines
            assertEquals("GraphQL: 4123 of 5000 points left, resets at 14:05.", lines[1])
            assertEquals("REST: 5000 requests, full again (the hourly window has reset).", lines[2])
            assertEquals(
                "GraphQL: not reported yet.",
                budgetExplanation(budget(null), BudgetSharing.PastedToken, now, utc).lines[1],
            )
        }

        @Test
        fun `the pace says how refreshing adapts`() {
            fun pace(schedule: PollSchedule) =
                budgetExplanation(budget(window(800, BudgetLevel.Low), schedule), BudgetSharing.PastedToken, now, utc).lines[3]
            assertEquals("Refreshing every 1 min while the feed is open.", pace(normal))
            assertEquals(
                "Refreshing slowed to every 2 min (from 1 min) so the budget lasts until the reset.",
                pace(PollSchedule(120, 60, PollPace.Stretched(BudgetLevel.Low))),
            )
            assertEquals(
                "Refreshing slowed to every 4 min (from 1 min) so the budget lasts until the reset — under 10% is left.",
                pace(PollSchedule(240, 60, PollPace.Stretched(BudgetLevel.Critical))),
            )
            assertEquals(
                "Too little is left for a refresh: the next one waits for the reset.",
                pace(PollSchedule(2100, 60, PollPace.UntilReset)),
            )
            assertEquals(
                "GitHub refused requests: the hourly budget is spent. Nothing is sent until 14:05, background notification checks included.",
                pace(paused(resets, PauseReason.PrimaryLimit)),
            )
            assertTrue(pace(paused(resets, PauseReason.SecondaryLimit)).startsWith("GitHub refused requests for coming too fast"))
        }

        @Test
        fun `intervals read in minutes when whole`() {
            assertEquals("1 min", intervalText(60))
            assertEquals("90 s", intervalText(90))
            assertEquals("30 s", intervalText(30))
        }
    }
}
