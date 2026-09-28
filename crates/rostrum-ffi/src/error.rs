//! The one error type every exported function returns.
//!
//! Kotlin sees it as `RostrumException`, a sealed class with one subclass per
//! variant, so a `when` over it is exhaustive. No variant has a field named
//! `message`: Kotlin exceptions already have one, and the generated class
//! would not compile. Human-readable text is `describe()`.

use std::time::SystemTime;

/// Everything that can go wrong in the core, by what the UI should do about it.
#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum RostrumError {
    /// No GitHub token has been handed in. Show the sign-in screen.
    #[error("no GitHub token is set")]
    NotSignedIn,

    /// GitHub answered 401: the token is revoked, expired, or mistyped.
    #[error("GitHub rejected the token: {reason}")]
    GitHubAuthFailed { reason: String },

    /// The primary or secondary rate limit is exhausted until `resets_at`.
    #[error("GitHub's rate limit is exhausted until {resets_at:?}")]
    GitHubRateLimited { resets_at: SystemTime },

    /// GitHub refused a merge (protection, failing checks, a moved head, a
    /// conflict). `reason` is GitHub's own explanation.
    #[error("GitHub refused the merge: {reason}")]
    MergeBlocked { reason: String },

    /// Any other refusal or unexpected answer from GitHub. `status` is the
    /// HTTP status when there was one; a GraphQL-level error arrives with
    /// HTTP 200 and is reported with no status.
    #[error("GitHub error{}: {reason}", status.map(|s| format!(" {s}")).unwrap_or_default())]
    GitHubApi { status: Option<u16>, reason: String },

    /// GitHub could not be reached at all.
    #[error("network error: {reason}")]
    Network { reason: String },

    /// The pull request is not in the feed or the local cache. Refresh the
    /// feed first.
    #[error("{repo}#{number} is not in the feed")]
    UnknownPullRequest { repo: String, number: u32 },

    /// The pending review was written against `drafted_against`, and the pull
    /// request's head has since moved to `head`. Its line anchors may point at
    /// the wrong lines, so it cannot be submitted or added to. Discard it.
    #[error("the pending review was written against {drafted_against}, but the head is now {head}")]
    DraftsStale {
        drafted_against: String,
        head: String,
    },

    /// No desktop is paired, or `set_remote` has not been called this session.
    #[error("not paired with a desktop")]
    NotPaired,

    /// The desktop answered 401: this device was unpaired there.
    #[error("the desktop no longer recognises this device")]
    DeviceRevoked,

    /// None of the desktop's addresses could be connected to.
    #[error("the desktop did not answer: {reason}")]
    DesktopUnreachable { reason: String },

    /// A host answered with a certificate other than the paired one. Either
    /// the desktop regenerated its certificate or something is intercepting.
    #[error("{host} presented a different certificate than the one this phone paired with")]
    CertificateMismatch { host: String },

    /// The desktop accepted the request but did not answer in time. A job may
    /// still be running there.
    #[error("the desktop took too long to answer")]
    DesktopTimeout,

    /// The desktop speaks a protocol version this build does not.
    #[error("the desktop speaks API version {desktop}, this app speaks {supported}")]
    IncompatibleDesktop { desktop: u32, supported: u32 },

    /// The desktop refused the request with a structured error.
    #[error("the desktop refused the request ({code:?}): {reason}")]
    RemoteApi {
        code: RemoteErrorCode,
        reason: String,
    },

    /// The desktop answered with something that is not the protocol.
    #[error("unexpected response from the desktop: {reason}")]
    RemoteProtocol { reason: String },

    /// `add_repo` input is not `owner/name` or a GitHub URL.
    #[error("`{input}` is not a repository: {reason}")]
    InvalidRepo { input: String, reason: String },

    /// `add_repo` input names a repository already in the list.
    #[error("{repo} is already in the list")]
    DuplicateRepo { repo: String },

    /// An argument was malformed or out of range.
    #[error("{reason}")]
    InvalidInput { reason: String },

    /// The local database or config file could not be read or written.
    #[error("local storage failed: {reason}")]
    Storage { reason: String },

    /// Placeholder for API that is declared but not built yet.
    #[error("{what} is not implemented yet")]
    Unimplemented { what: String },

    /// A bug: a background task died or an invariant broke.
    #[error("internal error: {reason}")]
    Internal { reason: String },
}

#[uniffi::export]
impl RostrumError {
    /// A sentence for the UI, in the core's own words.
    pub fn describe(&self) -> String {
        self.to_string()
    }
}

/// Why the desktop refused a request. Mirrors the protocol's error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteErrorCode {
    Unauthorized,
    Forbidden,
    BadRequest,
    NotFound,
    PairingCodeInvalid,
    PairingCodeExpired,
    RateLimited,
    Busy,
    Internal,
}

impl RostrumError {
    pub(crate) fn unimplemented(what: &str) -> Self {
        Self::Unimplemented { what: what.into() }
    }

    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            reason: reason.into(),
        }
    }

    pub(crate) fn internal(reason: impl Into<String>) -> Self {
        Self::Internal {
            reason: reason.into(),
        }
    }
}
