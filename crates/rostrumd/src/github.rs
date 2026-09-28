//! Handing the desktop's GitHub token to a paired phone.
//!
//! The token is resolved fresh on each request the same way the desktop app
//! resolves it (`gh auth token`, then `$GITHUB_TOKEN`), so a `gh auth
//! refresh` on the desktop reaches the phone the next time it asks. Nothing
//! here stores or logs it.

use rostrum_remote::{GitHubHandover, GitHubToken};

use crate::boxed::BoxFuture;

pub const GITHUB_HOST: &str = "github.com";

/// Where a [`GitHubHandover`] comes from. The daemon uses [`GhHandover`];
/// tests supply a fixed answer.
pub trait HandoverSource: Send + Sync {
    /// The current token, or `None` when the desktop has none.
    fn handover(&self) -> BoxFuture<'_, Option<GitHubHandover>>;
}

/// `rostrum_github::resolve_token`, labelled with the machine it came from.
pub struct GhHandover {
    machine: String,
}

impl GhHandover {
    pub fn new(machine: impl Into<String>) -> Self {
        Self {
            machine: machine.into(),
        }
    }
}

impl HandoverSource for GhHandover {
    fn handover(&self) -> BoxFuture<'_, Option<GitHubHandover>> {
        Box::pin(async move {
            match rostrum_github::resolve_token().await {
                Ok((token, source)) => Some(GitHubHandover {
                    token: GitHubToken::new(token.as_str()),
                    host: GITHUB_HOST.to_string(),
                    source: format!("{source} on {}", self.machine),
                }),
                Err(error) => {
                    tracing::debug!(%error, "no GitHub token to hand over");
                    None
                }
            }
        })
    }
}
