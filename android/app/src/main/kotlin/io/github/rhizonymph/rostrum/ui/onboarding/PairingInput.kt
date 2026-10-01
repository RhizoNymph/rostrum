package io.github.rhizonymph.rostrum.ui.onboarding

/** The port rostrumd's pairing API listens on by default. */
const val DEFAULT_PAIRING_PORT = 8485

/** A pairing code as typed: letters and digits only, uppercased, at most 8, grouped `XXXX-XXXX`. */
fun formatPairingCode(input: String): String {
    val raw = input.filter { it.isLetterOrDigit() }.uppercase().take(8)
    return if (raw.length > 4) raw.substring(0, 4) + "-" + raw.substring(4) else raw
}

/** Whether [code] (as formatted) holds all eight characters. */
fun isCompletePairingCode(code: String): Boolean = code.count { it.isLetterOrDigit() } == 8

/** A TCP port from text, or `null` unless it is 1..65535. */
fun parsePort(text: String): Int? = text.trim().toIntOrNull()?.takeIf { it in 1..65535 }

/** `192.168.1.24, nymph-desk.local :8485`. */
fun addressesLabel(hosts: List<String>, port: Int): String = hosts.joinToString(", ") + " :$port"
