package io.github.rhizonymph.rostrum.data.model

import java.time.Instant

/**
 * A profile's id, as the core's registry made it. Only ids of a safe shape
 * exist (letters, digits, `-` and `_`), since they name directories for the
 * profile's secrets.
 */
@JvmInline
value class ProfileId private constructor(val value: String) {
    override fun toString(): String = value

    companion object {
        private val SHAPE = Regex("[A-Za-z0-9_-]{1,64}")

        /** `null` when [raw] is not an id this app would ever have made. */
        fun of(raw: String): ProfileId? = if (SHAPE.matches(raw)) ProfileId(raw) else null
    }
}

/** What a profile is for. */
sealed interface ProfileKind {
    /** Paired with the desktop [machine]; its GitHub token usually came from there. */
    data class Desktop(val machine: String, val fingerprintShort: String) : ProfileKind

    /** A GitHub token pasted on the phone, no desktop. */
    data object TokenOnly : ProfileKind
}

/**
 * One profile: its own repositories, filters, cache, drafts and GitHub
 * account. [githubLogin] is who its token belongs to, when known.
 */
data class Profile(
    val id: ProfileId,
    val label: String,
    val kind: ProfileKind,
    val githubLogin: String?,
    val createdAt: Instant,
    val lastUsed: Instant,
)

/**
 * The registry paired a desktop: into a new profile ([created]), or again
 * into the profile already paired with that desktop's certificate.
 */
data class ProfilePairing(
    val profile: Profile,
    val created: Boolean,
    val pairing: PairingResult,
)
