//! The protocol between a paired phone and `rostrumd` on the user's desktop.
//!
//! The desktop owns the local clones; the phone owns nothing but a device token
//! and the fingerprint of the desktop's self-signed certificate. Everything
//! that crosses between them is a type in this crate, so the server and the
//! client cannot disagree about a field name.
//!
//! - [`pairing`]: the one-time code, the `rostrum://pair` link that carries it,
//!   and the exchange that turns it into a device token.
//! - [`api`]: the authenticated requests a paired phone makes — local state,
//!   local jobs, sync-all, handoff sessions.
//! - [`client`] (feature `client`): the phone's HTTPS client, which trusts one
//!   certificate by fingerprint and nothing else.
//!
//! Transport: HTTPS on the desktop's API port, `Authorization: Bearer <device
//! token>` on every route except [`routes::HELLO`] and [`routes::PAIR`], JSON
//! both ways. A successful call whose answer carries no data returns JSON
//! `null`; a failed one returns an [`api::ApiError`] with a matching status.

pub mod api;
pub mod code;
pub mod fingerprint;
pub mod host;
pub mod pairing;
pub mod secret;

#[cfg(feature = "client")]
pub mod client;

pub use api::{
    AbortRequest, ApiError, ApiErrorCode, CloneInfo, DesktopConfig, HandoffSession, HandoffStatus,
    InProgressKind, JobOutcome, JobRequest, LocalBranchStatus, LocalOpKind, LocalStatus,
    LocalStatusRequest, MachineInfo, PrKey, PrRef, SyncAllRequest, SyncEntry, SyncEntryState,
    SyncRun, SyncSummary,
};
pub use code::{PairingCode, PairingCodeError};
pub use fingerprint::{CertFingerprint, FingerprintError};
pub use host::{Host, HostError};
pub use pairing::{
    Endpoint, EndpointError, GitHubHandover, Hello, PairRequest, PairResponse, PairingOffer,
    PairingUriError,
};
pub use secret::{DeviceId, DeviceToken, GitHubToken, SecretError, TokenHash};

/// Bumped when a route or a type changes incompatibly. The phone refuses to
/// pair with a desktop whose [`pairing::Hello::api_version`] it does not speak.
pub const API_VERSION: u32 = 1;

/// Every route the API serves, relative to `https://<host>:<port>`.
pub mod routes {
    /// `GET` → [`crate::Hello`]. Unauthenticated: reachability and version.
    pub const HELLO: &str = "/api/v1/hello";
    /// `POST` [`crate::PairRequest`] → [`crate::PairResponse`]. Unauthenticated;
    /// the pairing code is the credential.
    pub const PAIR: &str = "/api/v1/pair";
    /// `GET` → [`crate::MachineInfo`].
    pub const MACHINE: &str = "/api/v1/machine";
    /// `GET` → [`crate::DesktopConfig`]: the repositories and feed preferences
    /// a phone may copy.
    pub const CONFIG: &str = "/api/v1/config";
    /// `GET` → [`crate::GitHubHandover`]: the desktop's current GitHub token,
    /// for a phone whose copy stopped working.
    pub const GITHUB_TOKEN: &str = "/api/v1/github-token";
    /// `POST` [`crate::LocalStatusRequest`] → [`crate::LocalStatus`].
    pub const LOCAL_STATUS: &str = "/api/v1/local/status";
    /// `POST` [`crate::JobRequest`] → [`crate::JobOutcome`]. Runs to completion
    /// before answering.
    pub const LOCAL_JOB: &str = "/api/v1/local/job";
    /// `POST` [`crate::AbortRequest`] → `null`.
    pub const LOCAL_ABORT: &str = "/api/v1/local/abort";
    /// `POST` [`crate::SyncAllRequest`] → [`crate::SyncRun`] (just started);
    /// `GET` → `Option<`[`crate::SyncRun`]`>` (the latest, running or finished).
    pub const SYNC_ALL: &str = "/api/v1/sync-all";
    /// `GET` → `Vec<`[`crate::HandoffSession`]`>`.
    pub const HANDOFFS: &str = "/api/v1/handoffs";
    /// `DELETE` → `null`: forget the calling device.
    pub const DEVICE: &str = "/api/v1/device";
}
