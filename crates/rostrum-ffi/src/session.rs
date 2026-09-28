//! The GitHub session: the token Kotlin hands in, and who it belongs to.

use crate::{engine::RostrumCore, error::RostrumError, types::UserRef};

/// What the core knows about the GitHub token.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum GitHubStatus {
    /// No token has been handed in.
    NoToken,
    /// A token is set but has not been used successfully yet.
    Unverified,
    /// A request with this token succeeded; it belongs to `viewer`.
    Verified { viewer: UserRef },
    /// GitHub rejected the token.
    Invalid { reason: String },
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Hand in the GitHub token (a pasted personal access token, or the one
    /// the paired desktop handed over), or `None` to sign out. Held in memory
    /// only; Kotlin persists it. Blank text counts as `None`.
    pub async fn set_github_token(
        &self,
        token: Option<String>,
    ) -> Result<GitHubStatus, RostrumError> {
        let _ = token;
        Err(RostrumError::unimplemented("set_github_token"))
    }

    /// The token's current status, without a network request.
    pub async fn github_status(&self) -> GitHubStatus {
        GitHubStatus::NoToken
    }

    /// Who the token belongs to. Answered from memory once known, otherwise
    /// asks GitHub — which also verifies a freshly pasted token.
    pub async fn viewer(&self) -> Result<UserRef, RostrumError> {
        Err(RostrumError::unimplemented("viewer"))
    }
}
