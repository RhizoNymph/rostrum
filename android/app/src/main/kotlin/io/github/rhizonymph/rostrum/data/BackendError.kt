package io.github.rhizonymph.rostrum.data

import io.github.rhizonymph.rostrum.data.model.StackRewrite
import java.time.Instant

/**
 * Everything that can go wrong in the backend, by what the UI should do about
 * it. Mirrors the core's `RostrumError` (Kotlin: `RostrumException`) variant
 * for variant, so the adapter over the generated bindings maps each generated
 * subclass onto exactly one of these.
 */
sealed interface BackendError {
    /** No GitHub token has been handed in. Show the sign-in screen. */
    data object NotSignedIn : BackendError

    /** GitHub answered 401: the token is revoked, expired, or mistyped. */
    data class GitHubAuthFailed(val reason: String) : BackendError

    /** The rate limit is exhausted until [resetsAt]. */
    data class GitHubRateLimited(val resetsAt: Instant) : BackendError

    /** GitHub refused a merge; [reason] is its own explanation. */
    data class MergeBlocked(val reason: String) : BackendError

    /** Any other refusal from GitHub. [status] is the HTTP status, if any. */
    data class GitHubApi(val status: Int?, val reason: String) : BackendError

    /** GitHub could not be reached at all. */
    data class Network(val reason: String) : BackendError

    /** The pull request is not in the feed or the cache. */
    data class UnknownPullRequest(val repo: String, val number: Int) : BackendError

    /** The pending review was written against an older head. Discard it. */
    data class DraftsStale(val draftedAgainst: String, val head: String) : BackendError

    /**
     * The issue's title or description changed on GitHub since the edit
     * began; [title], [body] and [updatedAt] are GitHub's now. Reload to take
     * them, or resend with overwrite.
     */
    data class EditConflict(val title: String, val body: String, val updatedAt: Instant) : BackendError {
        override fun toString(): String = "EditConflict(updatedAt=$updatedAt)"
    }

    /**
     * The desktop would rewrite other branches than the ones confirmed;
     * [branches] are what it would rewrite now.
     */
    data class RewriteNotConfirmed(val branches: List<StackRewrite>, val reason: String) : BackendError

    /** No desktop is paired for this session. */
    data object NotPaired : BackendError

    /** The desktop answered 401: this device was unpaired there. */
    data object DeviceRevoked : BackendError

    /** None of the desktop's addresses could be connected to. */
    data class DesktopUnreachable(val reason: String) : BackendError

    /** A host presented a different certificate than the paired one. */
    data class CertificateMismatch(val host: String) : BackendError

    /** The desktop accepted the request but did not answer in time. */
    data object DesktopTimeout : BackendError

    /** The desktop speaks a protocol version this build does not. */
    data class IncompatibleDesktop(val desktop: Int, val supported: Int) : BackendError

    /** The desktop refused the request with a structured error. */
    data class RemoteApi(val code: RemoteErrorCode, val reason: String) : BackendError

    /** The desktop answered with something that is not the protocol. */
    data class RemoteProtocol(val reason: String) : BackendError

    /** addRepo input is not `owner/name` or a GitHub URL. */
    data class InvalidRepo(val input: String, val reason: String) : BackendError

    /** addRepo input names a repository already in the list. */
    data class DuplicateRepo(val repo: String) : BackendError

    /** An argument was malformed or out of range. */
    data class InvalidInput(val reason: String) : BackendError

    /** No profile has this id (it was removed meanwhile). */
    data class ProfileNotFound(val id: String) : BackendError

    /** The local database, config file or secret store failed. */
    data class Storage(val reason: String) : BackendError

    /** A bug: a background task died or an invariant broke. */
    data class Internal(val reason: String) : BackendError
}

/** Why the desktop refused a request. Mirrors the protocol's error codes. */
enum class RemoteErrorCode {
    Unauthorized,
    Forbidden,
    BadRequest,
    NotFound,
    PairingCodeInvalid,
    PairingCodeExpired,
    RateLimited,
    Busy,
    RewriteNotConfirmed,
    Internal,
}

/** A sentence for the UI. */
fun BackendError.describe(): String = when (this) {
    BackendError.NotSignedIn -> "You're not signed in to GitHub."
    is BackendError.GitHubAuthFailed -> "GitHub rejected the token: $reason"
    is BackendError.GitHubRateLimited -> "GitHub's rate limit is used up until ${resetsAt.toLocalTimeText()}."
    is BackendError.MergeBlocked -> "GitHub refused the merge: $reason"
    is BackendError.GitHubApi -> if (status != null) "GitHub error $status: $reason" else "GitHub error: $reason"
    is BackendError.Network -> "Couldn't reach GitHub: $reason"
    is BackendError.UnknownPullRequest -> "$repo#$number isn't in the feed. Refresh and try again."
    is BackendError.DraftsStale ->
        "New commits were pushed after you drafted these comments (${draftedAgainst.take(7)} → ${head.take(7)})."
    BackendError.NotPaired -> "No desktop is paired."
    BackendError.DeviceRevoked -> "The desktop no longer recognises this phone. Pair again."
    is BackendError.DesktopUnreachable -> "The desktop didn't answer: $reason"
    is BackendError.CertificateMismatch ->
        "$host presented a different certificate than the one this phone paired with."
    BackendError.DesktopTimeout -> "The desktop took too long to answer. The job may still be running there."
    is BackendError.IncompatibleDesktop ->
        "The desktop speaks API version $desktop; this app speaks $supported. Update the older one."
    is BackendError.RemoteApi -> when (code) {
        RemoteErrorCode.PairingCodeInvalid -> "That pairing code isn't right."
        RemoteErrorCode.PairingCodeExpired -> "That pairing code has expired. Show a new one on the desktop."
        RemoteErrorCode.Busy -> "The desktop is busy with another job."
        RemoteErrorCode.RewriteNotConfirmed -> "The desktop needs you to confirm the branches it rewrites: $reason"
        else -> "The desktop refused: $reason"
    }
    is BackendError.EditConflict -> "This issue was changed on GitHub while you edited it."
    is BackendError.RewriteNotConfirmed -> "The desktop would rewrite other branches than you confirmed: $reason"
    is BackendError.RemoteProtocol -> "Unexpected answer from the desktop: $reason"
    is BackendError.InvalidRepo -> "$input isn't a repository: $reason"
    is BackendError.DuplicateRepo -> "$repo is already in your feed"
    is BackendError.InvalidInput -> reason
    is BackendError.ProfileNotFound -> "That profile no longer exists on this phone."
    is BackendError.Storage -> "Local storage failed: $reason"
    is BackendError.Internal -> "Something went wrong: $reason"
}

/** Errors that mean the GitHub token is unusable and the user must sign in. */
val BackendError.requiresSignIn: Boolean
    get() = this is BackendError.NotSignedIn || this is BackendError.GitHubAuthFailed

/** Errors that mean the pairing is gone and the desktop must be paired again. */
val BackendError.requiresPairing: Boolean
    get() = this is BackendError.NotPaired || this is BackendError.DeviceRevoked

private fun Instant.toLocalTimeText(): String =
    java.time.format.DateTimeFormatter.ofPattern("HH:mm")
        .withZone(java.time.ZoneId.systemDefault())
        .format(this)
