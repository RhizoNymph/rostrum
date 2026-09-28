package io.github.rhizonymph.rostrum.data.model

/**
 * Types shared by more than one area of the backend. They mirror the records
 * of `rostrum-ffi` (`types.rs`) one to one, with Kotlin-friendly numbers
 * (`Int`/`Long` for `u32`/`u64`, [java.time.Instant] for `SystemTime`), so the
 * adapter over the generated bindings is a mechanical mapping.
 */

/** One pull request, addressed the way every backend call takes it. */
data class PrRef(val repo: String, val number: Int) {
    init {
        require(number > 0) { "pull request numbers start at 1, got $number" }
    }

    override fun toString(): String = "$repo#$number"
}

/** A GitHub account as the UI shows it. */
data class UserRef(val login: String, val avatarUrl: String? = null)

/** What a coloured element means; the UI maps roles onto its palette. */
enum class ColorRole { Success, Warning, Danger, Draft, Accent, Neutral }

/** A short coloured tag: `conflict`, `↓3`, `approved`, `handed off`. */
data class Chip(val text: String, val role: ColorRole, val tooltip: String? = null)

/**
 * A label as GitHub defines it. [argb] is opaque ARGB, or `null` when GitHub's
 * hex value did not parse (render it neutral).
 */
data class LabelView(val name: String, val argb: Int?)

/** Which side of a diff a line or comment belongs to (GitHub's terms). */
enum class Side { Left, Right }

/** Rolled-up CI state of a commit, or of one check. */
enum class CheckState { Expected, Error, Failure, Pending, Success }

/** GitHub's aggregate review verdict on a pull request. */
enum class ReviewDecision { Approved, ChangesRequested, ReviewRequired }

/** Whether a pull request can be merged, and if not, why. */
enum class MergeStatus { Computing, Conflicts, Draft, Blocked, Behind, Unstable, Ready }

/** The state of one submitted review. */
enum class ReviewState { Pending, Commented, Approved, ChangesRequested, Dismissed }

/** Where a pull request is in its life. */
enum class PullState { Open, Closed, Merged }
