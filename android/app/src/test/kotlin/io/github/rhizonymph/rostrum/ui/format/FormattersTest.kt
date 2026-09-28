package io.github.rhizonymph.rostrum.ui.format

import io.github.rhizonymph.rostrum.data.model.Side
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import java.time.Duration
import java.time.Instant

class FormattersTest {
    private val now = Instant.parse("2026-09-28T12:00:00Z")

    @ParameterizedTest
    @CsvSource(
        "0, now", "59, now", "60, 1m", "720, 12m", "3600, 1h", "7200, 2h",
        "86400, 1d", "604800, 1w", "2592000, 4w", "3110400, 1mo", "31536000, 1y",
    )
    fun `relative ages`(seconds: Long, expected: String) {
        assertEquals(expected, relativeAge(now.minusSeconds(seconds), now))
    }

    @Test
    fun `future times read as now`() {
        assertEquals("now", relativeAge(now.plusSeconds(600), now))
        assertEquals("just now", relativeAgo(now, now))
        assertEquals("3m ago", relativeAgo(now.minusSeconds(180), now))
    }

    @ParameterizedTest
    @CsvSource("ada-lin, AL", "RhizoNymph, RN", "tjvance, TJ", "mkowal, MK", "sofia_r, SR", "x, X", "dmitri.k, DK")
    fun initials(login: String, expected: String) {
        assertEquals(expected, initials(login))
    }

    @Test
    fun `repositories split after the owner`() {
        assertEquals("RhizoNymph/" to "rostrum", splitRepo("RhizoNymph/rostrum"))
        assertEquals("" to "solo", splitRepo("solo"))
    }

    @Test
    fun `counts use the real minus sign`() {
        assertEquals("+900", additionsText(900))
        assertEquals("−9", deletionsText(9))
    }

    @Test
    fun `line ranges and sides`() {
        assertEquals("L60", lineRangeLabel(null, 60))
        assertEquals("L60", lineRangeLabel(60, 60))
        assertEquals("L53–56", lineRangeLabel(53, 56))
        assertEquals("new side", sideLabel(Side.Right))
        assertEquals("old side", sideLabel(Side.Left))
    }

    @Test
    fun `paths split into directory and name`() {
        assertEquals("overview.rs", fileName("crates/rostrum-diff/src/overview.rs"))
        assertEquals("crates/rostrum-diff/src/", directoryOf("crates/rostrum-diff/src/overview.rs"))
        assertEquals("", directoryOf("Cargo.toml"))
    }

    @Test
    fun `durations`() {
        assertEquals("58s", durationText(Duration.ofSeconds(58)))
        assertEquals("4m 12s", durationText(Duration.ofSeconds(252)))
        assertEquals("1h 03m", durationText(Duration.ofMinutes(63)))
    }

    @Test
    fun `labels`() {
        assertEquals("1 file", countLabel(1, "file"))
        assertEquals("7 files", countLabel(7, "file"))
        assertEquals("abcdef1", shortSha("abcdef1234"))
    }
}
