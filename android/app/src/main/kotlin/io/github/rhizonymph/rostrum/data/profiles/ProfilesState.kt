package io.github.rhizonymph.rostrum.data.profiles

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.session.SessionRepository

/** The profiles this phone has, and which one the app shows. */
sealed interface ProfilesState {
    /** The registry is being opened and the legacy state cleared. */
    data object Starting : ProfilesState

    /** The registry could not be opened; nothing can be shown. */
    data class Unavailable(val error: BackendError) : ProfilesState

    /**
     * [profiles] most recently used first. [active] is one of them, or `null`
     * when there are none (and, briefly, while the first pairing asks about
     * copying the desktop's settings).
     */
    data class Ready(val profiles: List<Profile>, val active: ProfileId?) : ProfilesState {
        init {
            require(active == null || profiles.any { it.id == active }) { "the active profile $active is not in the list" }
        }

        val activeProfile: Profile? get() = active?.let(::profile)

        fun profile(id: ProfileId): Profile? = profiles.firstOrNull { it.id == id }
    }
}

/** One profile's backend and session. [ProfileManager] makes one per profile id. */
class ProfileHandle(
    val id: ProfileId,
    val backend: RostrumBackend,
    val session: SessionRepository,
)

/** A pairing kept: into a new profile ([created]) or the one already paired with that desktop. */
data class PairedProfile(
    val profile: Profile,
    val created: Boolean,
    val machine: String,
)

/** What removing a profile left active. */
sealed interface ProfileRemoval {
    val removed: Profile

    /** It wasn't the active profile; nothing else changed. */
    data class Inactive(override val removed: Profile) : ProfileRemoval

    /** It was active; [next] (the most recently used of the rest) is now. */
    data class ActiveReplaced(override val removed: Profile, val next: Profile) : ProfileRemoval

    /** It was the last one; the app goes back to sign-in. */
    data class LastRemoved(override val removed: Profile) : ProfileRemoval
}
