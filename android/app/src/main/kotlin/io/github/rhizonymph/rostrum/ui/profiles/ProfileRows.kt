package io.github.rhizonymph.rostrum.ui.profiles

import io.github.rhizonymph.rostrum.data.model.Profile
import io.github.rhizonymph.rostrum.data.model.ProfileId
import io.github.rhizonymph.rostrum.data.model.ProfileKind
import io.github.rhizonymph.rostrum.data.profiles.ProfileRemoval
import io.github.rhizonymph.rostrum.data.profiles.ProfilesState

/** One profile as the switcher and Settings list it. */
data class ProfileRow(
    val id: ProfileId,
    val label: String,
    /** The desktop's machine name, or "GitHub token". */
    val kind: String,
    /** Who its GitHub token belongs to, when known. */
    val login: String?,
    val active: Boolean,
) {
    /** `nymph-desk · @rhizonymph`: the line under the label. */
    val detail: String get() = listOfNotNull(kind, login?.let { "@$it" }).joinToString(" · ")
}

fun ProfileKind.describe(): String = when (this) {
    is ProfileKind.Desktop -> machine
    ProfileKind.TokenOnly -> "GitHub token"
}

/** Every profile, most recently used first; none while the profiles load. */
fun profileRows(state: ProfilesState): List<ProfileRow> {
    val ready = state as? ProfilesState.Ready ?: return emptyList()
    return ready.profiles.map { profile ->
        ProfileRow(profile.id, profile.label, profile.kind.describe(), profile.githubLogin, profile.id == ready.active)
    }
}

/** The texts around removing a profile. */
object ProfileText {
    fun removeTitle(profile: Profile): String = "Remove ${profile.label}?"

    /**
     * What removing [profile] does; [next] is the profile that becomes active
     * when [profile] is the active one (`null` when it is the last).
     */
    fun removeBody(profile: Profile, active: Boolean, next: Profile?): String {
        val data = "its repositories, filters, cache, drafts and GitHub token"
        val what = when (val kind = profile.kind) {
            is ProfileKind.Desktop -> "This unpairs ${kind.machine} and deletes this phone's data for it: $data."
            ProfileKind.TokenOnly -> "This deletes this phone's data for it: $data."
        }
        val then = when {
            !active -> ""
            next != null -> " Rostrum switches to ${next.label}."
            else -> " You'll be back at sign-in."
        }
        return what + then
    }

    fun removed(removal: ProfileRemoval): String = when (removal) {
        is ProfileRemoval.Inactive -> "Removed ${removal.removed.label}"
        is ProfileRemoval.ActiveReplaced -> "Removed ${removal.removed.label}. Now using ${removal.next.label}"
        is ProfileRemoval.LastRemoved -> "Removed ${removal.removed.label}"
    }

    fun switched(profile: Profile): String = "Switched to ${profile.label}"
}
