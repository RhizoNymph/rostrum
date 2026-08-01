//! Domain types describing repositories and pull requests.

use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A repository identifier, always in `owner/name` form.
///
/// Construction is validated, so a `RepoId` in hand is always well-formed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepoId {
    owner: String,
    name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseRepoIdError {
    #[error("expected `owner/name`, got `{0}`")]
    Shape(String),
    #[error("`{0}` has an empty owner or name")]
    EmptyComponent(String),
}

impl RepoId {
    /// Build from already-validated parts.
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for RepoId {
    type Err = ParseRepoIdError;

    /// Accepts `owner/name`, and also tolerates a pasted GitHub URL or a
    /// trailing `.git`, since both are common when adding a repo by hand.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let without_scheme = trimmed
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("github.com/")
            .trim_start_matches("www.github.com/");
        let without_suffix = without_scheme
            .trim_end_matches('/')
            .trim_end_matches(".git");

        let mut parts = without_suffix.split('/');
        let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(ParseRepoIdError::Shape(trimmed.to_string()));
        };

        if owner.is_empty() || name.is_empty() {
            return Err(ParseRepoIdError::EmptyComponent(trimmed.to_string()));
        }

        Ok(Self::new(owner, name))
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A pull request number, unique within its repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PrNumber(pub u32);

impl fmt::Display for PrNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub login: String,
    pub avatar_url: Option<String>,
}

/// Aggregate review state. `None` on the wire means "no decision yet", which we
/// represent as `Option::None` rather than an extra variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// Rolled-up CI state for the head commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
}

impl CheckState {
    /// Whether this state should block a merge affordance.
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Error | Self::Failure)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    /// GitHub is still computing the merge state; not an error.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    /// Six-digit hex, no leading `#`, as GitHub returns it.
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: PrNumber,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author: Option<User>,
    pub head_ref: String,
    pub base_ref: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub mergeable: Mergeable,
    pub review_decision: Option<ReviewDecision>,
    pub labels: Vec<Label>,
    pub comment_count: u32,
    pub checks: Option<CheckState>,
}

impl PullRequest {
    /// Text used for feed filtering. Kept here so filtering and display can
    /// never drift apart.
    pub fn matches_query(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        self.title.to_lowercase().contains(&needle)
            || self.number.0.to_string().contains(&needle)
            || self
                .author
                .as_ref()
                .is_some_and(|a| a.login.to_lowercase().contains(&needle))
            || self
                .labels
                .iter()
                .any(|l| l.name.to_lowercase().contains(&needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_owner_name() {
        let id: RepoId = "zed-industries/zed".parse().expect("should parse");
        assert_eq!(id.owner(), "zed-industries");
        assert_eq!(id.name(), "zed");
        assert_eq!(id.to_string(), "zed-industries/zed");
    }

    #[test]
    fn parses_pasted_url_forms() {
        for input in [
            "https://github.com/zed-industries/zed",
            "https://github.com/zed-industries/zed.git",
            "https://github.com/zed-industries/zed/",
            "github.com/zed-industries/zed",
            "  zed-industries/zed  ",
        ] {
            let id: RepoId = input.parse().unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(id.to_string(), "zed-industries/zed", "input: {input}");
        }
    }

    #[test]
    fn rejects_malformed() {
        assert!("zed".parse::<RepoId>().is_err());
        assert!("a/b/c".parse::<RepoId>().is_err());
        assert!("/zed".parse::<RepoId>().is_err());
        assert!("zed/".parse::<RepoId>().is_err());
        assert!("".parse::<RepoId>().is_err());
    }

    fn pr(title: &str, author: &str) -> PullRequest {
        PullRequest {
            number: PrNumber(1),
            title: title.into(),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: Some(User {
                login: author.into(),
                avatar_url: None,
            }),
            head_ref: "feature".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            review_decision: None,
            labels: vec![Label {
                name: "bug".into(),
                color: "d73a4a".into(),
            }],
            comment_count: 0,
            checks: None,
        }
    }

    #[test]
    fn query_matches_title_author_and_label_case_insensitively() {
        let pr = pr("Fix the Thing", "RhizoNymph");
        assert!(pr.matches_query(""));
        assert!(pr.matches_query("thing"));
        assert!(pr.matches_query("rhizo"));
        assert!(pr.matches_query("BUG"));
        assert!(pr.matches_query("1"));
        assert!(!pr.matches_query("absent"));
    }
}
