//! The one error type every exported function returns.
//!
//! Kotlin sees it as `RostrumException`, a sealed class with one subclass per
//! variant, so a `when` over it is exhaustive. No variant has a field named
//! `message`: Kotlin exceptions already have one, and the generated class
//! would not compile. Human-readable text is `describe()`.

use std::time::{Duration, SystemTime};

use rostrum_github::GitHubError;
use rostrum_remote::{ApiErrorCode, client::ClientError};

use crate::stack_actions::StackRewrite;

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

    /// No profile has this id: it was removed, or never existed.
    #[error("there is no profile {id}")]
    ProfileNotFound { id: String },

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

    /// `edit_issue`: someone changed the issue's title or description after
    /// `base_updated_at`, and saving would discard it. Their version is here:
    /// show it, then reload it into the editor or call again with
    /// `overwrite = true`.
    #[error("the issue was edited elsewhere since it was opened")]
    EditConflict {
        title: String,
        body: String,
        updated_at: SystemTime,
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

    /// A stack request would rewrite branches it did not confirm (or
    /// confirms branches it would not rewrite). `branches` are what the
    /// desktop would rewrite now: show them, then send exactly their names as
    /// `confirm_rewrite`. Empty if the desktop could not be asked again.
    #[error("the desktop would rewrite other branches than the ones confirmed: {reason}")]
    RewriteNotConfirmed {
        branches: Vec<StackRewrite>,
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
    /// A stack request's `confirm_rewrite` did not match; stack calls report
    /// [`RostrumError::RewriteNotConfirmed`] instead.
    RewriteNotConfirmed,
    Internal,
}

impl RostrumError {
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

impl From<GitHubError> for RostrumError {
    fn from(error: GitHubError) -> Self {
        match error {
            GitHubError::NoToken(_) => Self::NotSignedIn,
            GitHubError::Unauthorized => Self::GitHubAuthFailed {
                reason: "the token was rejected (revoked, expired, or mistyped)".into(),
            },
            GitHubError::RateLimited { reset_at } => Self::GitHubRateLimited {
                resets_at: reset_at.into(),
            },
            GitHubError::SecondaryRateLimit { retry_after_secs } => Self::GitHubRateLimited {
                resets_at: SystemTime::now() + Duration::from_secs(retry_after_secs),
            },
            GitHubError::MergeBlocked { reason } => Self::MergeBlocked { reason },
            GitHubError::Forbidden { reason } => Self::GitHubApi {
                status: Some(403),
                reason,
            },
            GitHubError::NotFound { resource } => Self::GitHubApi {
                status: Some(404),
                reason: format!("{resource} not found, or not visible to this token"),
            },
            GitHubError::Unexpected { status, body } => Self::GitHubApi {
                status: Some(status),
                reason: body,
            },
            GitHubError::Network(error) => Self::Network {
                reason: root_cause(&error),
            },
            error @ (GitHubError::GraphQl { .. }
            | GitHubError::EmptyData
            | GitHubError::Decode { .. }) => Self::GitHubApi {
                status: None,
                reason: error.to_string(),
            },
        }
    }
}

impl From<ClientError> for RostrumError {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Unreachable { .. } => Self::DesktopUnreachable {
                reason: error.to_string(),
            },
            ClientError::CertificateMismatch { host } => Self::CertificateMismatch {
                host: host.to_string(),
            },
            ClientError::Unauthorized => Self::DeviceRevoked,
            ClientError::Api(api) => Self::RemoteApi {
                code: api.code.into(),
                reason: api.message,
            },
            ClientError::Timeout => Self::DesktopTimeout,
            ClientError::Protocol(reason) | ClientError::Tls(reason) => {
                Self::RemoteProtocol { reason }
            }
        }
    }
}

impl From<ApiErrorCode> for RemoteErrorCode {
    fn from(code: ApiErrorCode) -> Self {
        match code {
            ApiErrorCode::Unauthorized => Self::Unauthorized,
            ApiErrorCode::Forbidden => Self::Forbidden,
            ApiErrorCode::BadRequest => Self::BadRequest,
            ApiErrorCode::NotFound => Self::NotFound,
            ApiErrorCode::PairingCodeInvalid => Self::PairingCodeInvalid,
            ApiErrorCode::PairingCodeExpired => Self::PairingCodeExpired,
            ApiErrorCode::RateLimited => Self::RateLimited,
            ApiErrorCode::Busy => Self::Busy,
            // Stack calls turn this into `RostrumError::RewriteNotConfirmed`
            // with the branches; anything else reports the code.
            ApiErrorCode::RewriteNotConfirmed => Self::RewriteNotConfirmed,
            // The phone does not push settings through the FFI yet; until it
            // does, a stale push reads as the request being wrong.
            ApiErrorCode::ConfigChanged => Self::BadRequest,
            ApiErrorCode::Internal => Self::Internal,
        }
    }
}

impl From<rostrum_db::DbError> for RostrumError {
    fn from(error: rostrum_db::DbError) -> Self {
        Self::Storage {
            reason: chain(&error),
        }
    }
}

impl From<rostrum_config::ConfigError> for RostrumError {
    fn from(error: rostrum_config::ConfigError) -> Self {
        Self::Storage {
            reason: error.to_string(),
        }
    }
}

impl From<tokio::task::JoinError> for RostrumError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::internal(format!("background work did not complete: {error}"))
    }
}

/// The innermost cause: reqwest's own message is usually "error sending
/// request", which says nothing.
fn root_cause(error: &(dyn std::error::Error + 'static)) -> String {
    let mut current = error;
    while let Some(next) = current.source() {
        current = next;
    }
    current.to_string()
}

/// Every message in the chain, outermost first: `DbError`'s own message is a
/// category ("sqlite error"), and the detail is in its source.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut parts = vec![error.to_string()];
    let mut current = error.source();
    while let Some(next) = current {
        parts.push(next.to_string());
        current = next.source();
    }
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use rostrum_remote::{ApiError, Host};

    use super::*;

    #[test]
    fn github_failures_map_to_what_the_ui_does_about_them() {
        assert_eq!(
            RostrumError::from(GitHubError::NoToken("none".into())),
            RostrumError::NotSignedIn
        );
        assert!(matches!(
            RostrumError::from(GitHubError::Unauthorized),
            RostrumError::GitHubAuthFailed { .. }
        ));
        assert_eq!(
            RostrumError::from(GitHubError::MergeBlocked {
                reason: "checks failing".into()
            }),
            RostrumError::MergeBlocked {
                reason: "checks failing".into()
            }
        );
        assert_eq!(
            RostrumError::from(GitHubError::Unexpected {
                status: 422,
                body: "nope".into()
            }),
            RostrumError::GitHubApi {
                status: Some(422),
                reason: "nope".into()
            }
        );
        assert!(matches!(
            RostrumError::from(GitHubError::NotFound {
                resource: "a/b".into()
            }),
            RostrumError::GitHubApi {
                status: Some(404),
                ..
            }
        ));
        assert!(matches!(
            RostrumError::from(GitHubError::EmptyData),
            RostrumError::GitHubApi { status: None, .. }
        ));
    }

    #[test]
    fn rate_limits_carry_when_they_lift() {
        let reset = chrono::DateTime::from_timestamp(1_900_000_000, 0).expect("time");
        assert_eq!(
            RostrumError::from(GitHubError::RateLimited { reset_at: reset }),
            RostrumError::GitHubRateLimited {
                resets_at: reset.into()
            }
        );
        let RostrumError::GitHubRateLimited { resets_at } =
            RostrumError::from(GitHubError::SecondaryRateLimit {
                retry_after_secs: 60,
            })
        else {
            panic!("expected a rate limit");
        };
        assert!(resets_at > SystemTime::now());
    }

    #[test]
    fn desktop_failures_map_to_typed_variants() {
        assert_eq!(
            RostrumError::from(ClientError::Unauthorized),
            RostrumError::DeviceRevoked
        );
        assert_eq!(
            RostrumError::from(ClientError::Timeout),
            RostrumError::DesktopTimeout
        );
        let host: Host = "192.168.1.20".parse().expect("host");
        assert_eq!(
            RostrumError::from(ClientError::CertificateMismatch { host }),
            RostrumError::CertificateMismatch {
                host: "192.168.1.20".into()
            }
        );
        assert!(matches!(
            RostrumError::from(ClientError::Unreachable { failures: vec![] }),
            RostrumError::DesktopUnreachable { .. }
        ));
        assert_eq!(
            RostrumError::from(ClientError::Api(ApiError::new(
                ApiErrorCode::PairingCodeExpired,
                "expired"
            ))),
            RostrumError::RemoteApi {
                code: RemoteErrorCode::PairingCodeExpired,
                reason: "expired".into()
            }
        );
        assert!(matches!(
            RostrumError::from(ClientError::Tls("bad".into())),
            RostrumError::RemoteProtocol { .. }
        ));
    }

    #[test]
    fn describe_is_the_display_text() {
        let error = RostrumError::DuplicateRepo { repo: "a/b".into() };
        assert_eq!(error.describe(), "a/b is already in the list");
    }
}
