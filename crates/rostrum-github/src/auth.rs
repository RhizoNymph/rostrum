//! Token resolution.
//!
//! `gh` is the primary source: it already handles login, SSO, and refresh, so
//! rostrum stores no secrets of its own. Environment variables are the fallback
//! for machines without `gh` installed.

use std::fmt;

use tokio::process::Command;

use crate::error::GitHubError;

const ENV_VARS: [&str; 2] = ["GITHUB_TOKEN", "ROSTRUM_GITHUB_TOKEN"];

/// A GitHub API token.
///
/// `Debug` is redacted so a token can never reach logs through a derived
/// formatter or a panic message.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Safe-to-log form: prefix and last four characters only.
    pub fn redacted(&self) -> String {
        let len = self.0.chars().count();
        if len <= 8 {
            return "…".to_string();
        }
        let tail: String = self.0.chars().skip(len - 4).collect();
        format!("…{tail}")
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Token({})", self.redacted())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    GhCli,
    Env(&'static str),
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GhCli => write!(f, "gh auth token"),
            Self::Env(name) => write!(f, "${name}"),
        }
    }
}

/// Resolve a token from `gh`, then from the environment.
///
/// Returns [`GitHubError::NoToken`] with guidance when every source fails;
/// callers surface that as onboarding rather than a hard error.
pub async fn resolve_token() -> Result<(Token, TokenSource), GitHubError> {
    match gh_token().await {
        Ok(Some(token)) => return Ok((token, TokenSource::GhCli)),
        Ok(None) => tracing::debug!("gh produced no token; trying environment"),
        Err(err) => tracing::debug!(error = %err, "gh unavailable; trying environment"),
    }

    for name in ENV_VARS {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Ok((Token::new(trimmed), TokenSource::Env(name)));
            }
        }
    }

    Err(GitHubError::NoToken(format!(
        "`gh auth token` failed and none of {} are set. Run `gh auth login`.",
        ENV_VARS.join(", ")
    )))
}

/// Shell out to `gh auth token`. `Ok(None)` means gh ran but had no token.
async fn gh_token() -> Result<Option<Token>, std::io::Error> {
    let output = Command::new("gh").args(["auth", "token"]).output().await?;

    if !output.status.success() {
        tracing::debug!(
            status = ?output.status.code(),
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "gh auth token failed"
        );
        return Ok(None);
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!token.is_empty()).then(|| Token::new(token)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_the_token() {
        let token = Token::new("gho_supersecretvalue1234");
        let rendered = format!("{token:?}");
        assert!(!rendered.contains("supersecret"), "leaked: {rendered}");
        assert!(rendered.ends_with("1234)"), "unexpected: {rendered}");
    }

    #[test]
    fn short_tokens_redact_entirely() {
        assert_eq!(Token::new("abc").redacted(), "…");
        assert_eq!(Token::new("12345678").redacted(), "…");
    }
}
