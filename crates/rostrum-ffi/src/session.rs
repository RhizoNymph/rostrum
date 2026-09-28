//! The GitHub session: the token Kotlin hands in, and who it belongs to.
//!
//! The token lives here, in memory, and nowhere else in Rust. It is never
//! written to SQLite or the settings file and never logged (`Token`'s `Debug`
//! is redacted); Kotlin keeps the durable copy in the Keystore.

use rostrum_core::User;
use rostrum_github::{GitHubClient, Token};

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

/// The session's state. A client exists exactly when a token does.
pub(crate) enum Session {
    SignedOut,
    Active {
        token: Token,
        client: GitHubClient,
        trust: Trust,
    },
}

/// What GitHub has said about the token so far.
pub(crate) enum Trust {
    Unverified,
    Verified(User),
    Rejected(String),
}

impl Session {
    /// Replace the token. The same token again keeps what is known about it;
    /// a different one starts unverified, since it may belong to someone else.
    pub(crate) fn set_token(&mut self, token: Option<String>) -> Result<(), RostrumError> {
        let token = token
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty());
        let Some(raw) = token else {
            *self = Self::SignedOut;
            return Ok(());
        };
        if let Self::Active { token, .. } = self
            && token.as_str() == raw
        {
            return Ok(());
        }
        let token = Token::new(raw);
        let client = GitHubClient::new(token.clone())
            .map_err(|error| RostrumError::internal(format!("could not build a client: {error}")))?;
        tracing::info!(token = %token.redacted(), "github token set");
        *self = Self::Active {
            token,
            client,
            trust: Trust::Unverified,
        };
        Ok(())
    }

    pub(crate) fn client(&self) -> Option<GitHubClient> {
        match self {
            Self::SignedOut => None,
            Self::Active { client, .. } => Some(client.clone()),
        }
    }

    pub(crate) fn viewer(&self) -> Option<&User> {
        match self {
            Self::Active {
                trust: Trust::Verified(user),
                ..
            } => Some(user),
            _ => None,
        }
    }

    /// A request with the token succeeded, and says who it belongs to.
    pub(crate) fn verify(&mut self, viewer: User) {
        if let Self::Active { trust, .. } = self {
            *trust = Trust::Verified(viewer);
        }
    }

    /// GitHub answered 401 to the token.
    pub(crate) fn reject(&mut self, reason: String) {
        if let Self::Active { trust, .. } = self {
            tracing::warn!("github rejected the token");
            *trust = Trust::Rejected(reason);
        }
    }

    pub(crate) fn status(&self) -> GitHubStatus {
        match self {
            Self::SignedOut => GitHubStatus::NoToken,
            Self::Active { trust, .. } => match trust {
                Trust::Unverified => GitHubStatus::Unverified,
                Trust::Verified(user) => GitHubStatus::Verified {
                    viewer: UserRef::from(user),
                },
                Trust::Rejected(reason) => GitHubStatus::Invalid {
                    reason: reason.clone(),
                },
            },
        }
    }
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
        self.actor
            .try_call(move |state| {
                state.session.set_token(token)?;
                // The viewer, and so "yours" and "review requested", may have
                // changed with the token.
                state.publish();
                Ok(state.session.status())
            })
            .await
    }

    /// The token's current status, without a network request.
    pub async fn github_status(&self) -> GitHubStatus {
        self.actor
            .call(|state| state.session.status())
            .await
            .unwrap_or(GitHubStatus::NoToken)
    }

    /// Who the token belongs to. Answered from memory once known, otherwise
    /// asks GitHub — which also verifies a freshly pasted token.
    pub async fn viewer(&self) -> Result<UserRef, RostrumError> {
        let (known, client) = self
            .actor
            .try_call(|state| {
                Ok((
                    state.session.viewer().map(UserRef::from),
                    state.github()?,
                ))
            })
            .await?;
        if let Some(viewer) = known {
            return Ok(viewer);
        }
        let user = self.github(client.viewer().await).await?;
        let found = UserRef::from(&user);
        self.actor
            .call(move |state| {
                state.session.verify(user);
                state.publish();
            })
            .await?;
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(login: &str) -> User {
        User {
            login: login.into(),
            avatar_url: None,
        }
    }

    #[test]
    fn blank_tokens_sign_out() {
        let mut session = Session::SignedOut;
        session.set_token(Some("  ".into())).expect("set");
        assert_eq!(session.status(), GitHubStatus::NoToken);
        assert!(session.client().is_none());
    }

    #[test]
    fn a_token_starts_unverified_and_learns_its_viewer() {
        let mut session = Session::SignedOut;
        session.set_token(Some(" ghp_abc ".into())).expect("set");
        assert_eq!(session.status(), GitHubStatus::Unverified);
        assert!(session.client().is_some());

        session.verify(user("octocat"));
        assert_eq!(
            session.status(),
            GitHubStatus::Verified {
                viewer: UserRef {
                    login: "octocat".into(),
                    avatar_url: None
                }
            }
        );
        assert_eq!(session.viewer().map(|u| u.login.as_str()), Some("octocat"));
    }

    #[test]
    fn the_same_token_keeps_what_is_known_and_a_new_one_forgets_it() {
        let mut session = Session::SignedOut;
        session.set_token(Some("ghp_abc".into())).expect("set");
        session.verify(user("octocat"));

        session.set_token(Some("ghp_abc".into())).expect("set");
        assert!(session.viewer().is_some());

        session.set_token(Some("ghp_other".into())).expect("set");
        assert_eq!(session.status(), GitHubStatus::Unverified);
        assert!(session.viewer().is_none());
    }

    #[test]
    fn a_rejected_token_reports_why_and_signing_out_clears_it() {
        let mut session = Session::SignedOut;
        // Rejecting with no token is meaningless and changes nothing.
        session.reject("nope".into());
        assert_eq!(session.status(), GitHubStatus::NoToken);

        session.set_token(Some("ghp_abc".into())).expect("set");
        session.reject("revoked".into());
        assert_eq!(
            session.status(),
            GitHubStatus::Invalid {
                reason: "revoked".into()
            }
        );
        session.set_token(None).expect("set");
        assert_eq!(session.status(), GitHubStatus::NoToken);
    }
}
