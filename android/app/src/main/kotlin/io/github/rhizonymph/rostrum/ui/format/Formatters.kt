package io.github.rhizonymph.rostrum.ui.format

import io.github.rhizonymph.rostrum.data.model.Side
import java.time.Duration
import java.time.Instant

/**
 * Pure text formatting shared by every screen. Kept free of Android and
 * Compose so it is unit-tested on the JVM.
 */

/** The minus sign the mockups use in `−9` (U+2212), not a hyphen. */
const val MINUS = "−"

/** `2h`, `12m`, `1d`, `3w`, `4mo`, `2y`; `now` under a minute; future times read `now`. */
fun relativeAge(then: Instant, now: Instant): String {
    val age = Duration.between(then, now)
    if (age.isNegative || age < Duration.ofMinutes(1)) return "now"
    val minutes = age.toMinutes()
    val hours = age.toHours()
    val days = age.toDays()
    return when {
        minutes < 60 -> "${minutes}m"
        hours < 24 -> "${hours}h"
        days < 7 -> "${days}d"
        days < 35 -> "${days / 7}w"
        days < 365 -> "${days / 30}mo"
        else -> "${days / 365}y"
    }
}

/** `3m ago`, `just now`: [relativeAge] as a phrase. */
fun relativeAgo(then: Instant, now: Instant): String =
    relativeAge(then, now).let { if (it == "now") "just now" else "$it ago" }

/** Git's seven-character abbreviation. */
fun shortSha(sha: String): String = sha.take(7)

/**
 * Two letters for an avatar: the initials of the first two words (split on
 * `-`, `_`, `.` or camel humps), else the first two letters.
 * `ada-lin` → `AL`, `RhizoNymph` → `RN`, `tjvance` → `TJ`.
 */
fun initials(login: String): String {
    val words = login.split('-', '_', '.', ' ').filter { it.isNotEmpty() }
    if (words.size >= 2) return (words[0].take(1) + words[1].take(1)).uppercase()
    val single = words.firstOrNull() ?: return "?"
    val humps = single.drop(1).indexOfFirst { it.isUpperCase() }
    return if (humps >= 0) {
        (single.take(1) + single[humps + 1]).uppercase()
    } else {
        single.take(2).uppercase()
    }
}

/** `RhizoNymph/rostrum` → (`RhizoNymph/`, `rostrum`), for the two-weight repo name. */
fun splitRepo(repo: String): Pair<String, String> {
    val slash = repo.indexOf('/')
    return if (slash < 0) "" to repo else repo.substring(0, slash + 1) to repo.substring(slash + 1)
}

/** `+900`. */
fun additionsText(additions: Number): String = "+$additions"

/** `−9`, with the real minus sign. */
fun deletionsText(deletions: Number): String = "$MINUS$deletions"

/** `1 file`, `7 files`. */
fun countLabel(count: Int, singular: String, plural: String = singular + "s"): String =
    "$count ${if (count == 1) singular else plural}"

/** `L60` or `L53–56` (en dash). */
fun lineRangeLabel(start: Int?, end: Int): String =
    if (start == null || start == end) "L$end" else "L$start–$end"

/** `new side` / `old side`, as the line-comment anchor chip says it. */
fun sideLabel(side: Side): String = when (side) {
    Side.Right -> "new side"
    Side.Left -> "old side"
}

/** The file name of a path: `crates/a/overview.rs` → `overview.rs`. */
fun fileName(path: String): String = path.substringAfterLast('/')

/** The directory of a path with a trailing slash, or empty: `crates/a/overview.rs` → `crates/a/`. */
fun directoryOf(path: String): String = if ('/' in path) path.substringBeforeLast('/') + "/" else ""

/** `4m 12s`, `58s`, `1h 03m`. */
fun durationText(duration: Duration): String {
    val seconds = duration.seconds.coerceAtLeast(0)
    return when {
        seconds < 60 -> "${seconds}s"
        seconds < 3600 -> "${seconds / 60}m ${"%02d".format(seconds % 60)}s"
        else -> "${seconds / 3600}h ${"%02d".format((seconds % 3600) / 60)}m"
    }
}

/** A pull request's display reference: `RhizoNymph/rostrum #10`. */
fun prLabel(repo: String, number: Int): String = "$repo #$number"
