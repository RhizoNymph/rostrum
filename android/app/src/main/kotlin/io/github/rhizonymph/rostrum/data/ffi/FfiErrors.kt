package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RemoteErrorCode
import io.github.rhizonymph.rostrum.data.RostrumLog
import uniffi.rostrum_ffi.InternalException
import uniffi.rostrum_ffi.RostrumException
import uniffi.rostrum_ffi.RemoteErrorCode as FfiRemoteErrorCode

/** Every core error, variant for variant. The `when` is exhaustive over the sealed class. */
internal fun RostrumException.toBackendError(): BackendError = when (this) {
    is RostrumException.NotSignedIn -> BackendError.NotSignedIn
    is RostrumException.GitHubAuthFailed -> BackendError.GitHubAuthFailed(reason)
    is RostrumException.GitHubRateLimited -> BackendError.GitHubRateLimited(resetsAt)
    is RostrumException.MergeBlocked -> BackendError.MergeBlocked(reason)
    is RostrumException.GitHubApi -> BackendError.GitHubApi(status?.toInt(), reason)
    is RostrumException.Network -> BackendError.Network(reason)
    is RostrumException.UnknownPullRequest -> BackendError.UnknownPullRequest(repo, number.toInt())
    is RostrumException.DraftsStale -> BackendError.DraftsStale(draftedAgainst, head)
    is RostrumException.EditConflict -> BackendError.EditConflict(title, body, updatedAt)
    is RostrumException.RewriteNotConfirmed -> BackendError.RewriteNotConfirmed(branches.map { it.toModel() }, reason)
    is RostrumException.NotPaired -> BackendError.NotPaired
    is RostrumException.DeviceRevoked -> BackendError.DeviceRevoked
    is RostrumException.DesktopUnreachable -> BackendError.DesktopUnreachable(reason)
    is RostrumException.CertificateMismatch -> BackendError.CertificateMismatch(host)
    is RostrumException.DesktopTimeout -> BackendError.DesktopTimeout
    is RostrumException.IncompatibleDesktop -> BackendError.IncompatibleDesktop(desktop.toInt(), supported.toInt())
    is RostrumException.RemoteApi -> BackendError.RemoteApi(code.toModel(), reason)
    is RostrumException.RemoteProtocol -> BackendError.RemoteProtocol(reason)
    is RostrumException.InvalidRepo -> BackendError.InvalidRepo(input, reason)
    is RostrumException.DuplicateRepo -> BackendError.DuplicateRepo(repo)
    is RostrumException.InvalidInput -> BackendError.InvalidInput(reason)
    is RostrumException.ProfileNotFound -> BackendError.ProfileNotFound(id)
    is RostrumException.Storage -> BackendError.Storage(reason)
    is RostrumException.Internal -> BackendError.Internal(reason)
}

internal fun FfiRemoteErrorCode.toModel(): RemoteErrorCode = when (this) {
    FfiRemoteErrorCode.UNAUTHORIZED -> RemoteErrorCode.Unauthorized
    FfiRemoteErrorCode.FORBIDDEN -> RemoteErrorCode.Forbidden
    FfiRemoteErrorCode.BAD_REQUEST -> RemoteErrorCode.BadRequest
    FfiRemoteErrorCode.NOT_FOUND -> RemoteErrorCode.NotFound
    FfiRemoteErrorCode.PAIRING_CODE_INVALID -> RemoteErrorCode.PairingCodeInvalid
    FfiRemoteErrorCode.PAIRING_CODE_EXPIRED -> RemoteErrorCode.PairingCodeExpired
    FfiRemoteErrorCode.RATE_LIMITED -> RemoteErrorCode.RateLimited
    FfiRemoteErrorCode.BUSY -> RemoteErrorCode.Busy
    FfiRemoteErrorCode.REWRITE_NOT_CONFIRMED -> RemoteErrorCode.RewriteNotConfirmed
    FfiRemoteErrorCode.INTERNAL -> RemoteErrorCode.Internal
}

internal const val CORE_LOG_TAG = "RostrumCore"

/**
 * Run one call into the core. Its declared errors ([RostrumException]) and a
 * Rust panic surfaced by UniFFI ([InternalException]) become [Outcome.Err];
 * cancellation and anything else propagate unchanged. Inline, so [block] may
 * suspend when the caller does.
 */
internal inline fun <T> ffiCall(op: String, block: () -> T): Outcome<T> = try {
    Outcome.Ok(block())
} catch (e: RostrumException) {
    val error = e.toBackendError()
    RostrumLog.d(CORE_LOG_TAG, "core_call_failed", "op" to op, "error" to error::class.simpleName)
    Outcome.Err(error)
} catch (e: InternalException) {
    RostrumLog.e(CORE_LOG_TAG, "core_call_panicked", "op" to op, "reason" to e.message)
    Outcome.Err(BackendError.Internal(e.message ?: "the core failed unexpectedly"))
}
