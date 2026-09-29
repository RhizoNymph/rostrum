package io.github.rhizonymph.rostrum.ui.onboarding

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import org.junit.jupiter.api.Test

class PairingInputTest {
    @ParameterizedTest
    @CsvSource(
        "wdjbmjht, WDJB-MJHT",
        "WDJB-MJHT, WDJB-MJHT",
        "wd jb mj ht, WDJB-MJHT",
        "wdj, WDJ",
        "wdjb, WDJB",
        "wdjbm, WDJB-M",
        "wdjbmjhtxyz, WDJB-MJHT",
        "'', ''",
    )
    fun `codes are uppercased, grouped and capped`(input: String, expected: String) {
        assertEquals(expected, formatPairingCode(input))
    }

    @Test
    fun `ports`() {
        assertEquals(8485, parsePort("8485"))
        assertEquals(1, parsePort(" 1 "))
        assertNull(parsePort("0"))
        assertNull(parsePort("65536"))
        assertNull(parsePort("abc"))
        assertNull(parsePort(""))
    }

    @Test
    fun `addresses read as a list with the port`() {
        assertEquals("192.168.1.24, nymph-desk.local :8485", addressesLabel(listOf("192.168.1.24", "nymph-desk.local"), 8485))
    }
}
