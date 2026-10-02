//! A validated branch name, as GitHub reports it.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A branch name without its `refs/heads/` prefix — `main`, `release/1.2`.
///
/// Validated on construction against the parts of `git check-ref-format` a
/// hand-edited config can get wrong, so a `TrunkName` in hand is always one
/// GitHub could hold. That matters because trunk names come from a
/// human-edited file and from a text box, and travel to GitHub as GraphQL
/// variables: a name with a space in it would never resolve, and saying so at
/// the input beats a branch that silently reads as missing.
///
/// Serialises as the plain string, so the config file stays an array of
/// names a person can edit.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TrunkName(String);

/// Why a string is not a usable branch name.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TrunkNameError {
    #[error("a branch name cannot be empty")]
    Empty,
    #[error("`{0}` contains whitespace or a control character")]
    Whitespace(String),
    #[error("`{0}` contains a character git does not allow in a branch name")]
    ForbiddenCharacter(String),
    #[error("`{0}` is not a well-formed branch name")]
    Malformed(String),
}

impl TrunkName {
    /// Parse user input: surrounding whitespace is trimmed and a leading
    /// `refs/heads/` dropped, since both are what a person pastes.
    pub fn parse(raw: &str) -> Result<Self, TrunkNameError> {
        let trimmed = raw.trim();
        let name = trimmed.strip_prefix("refs/heads/").unwrap_or(trimmed);
        if name.is_empty() {
            return Err(TrunkNameError::Empty);
        }
        if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(TrunkNameError::Whitespace(name.to_string()));
        }
        if name
            .chars()
            .any(|c| matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
        {
            return Err(TrunkNameError::ForbiddenCharacter(name.to_string()));
        }
        let malformed = name.starts_with('-')
            || name.starts_with('/')
            || name.ends_with('/')
            || name.ends_with('.')
            || name.ends_with(".lock")
            || name.contains("..")
            || name.contains("//")
            || name.contains("@{")
            || name == "@"
            || name.split('/').any(|part| part.starts_with('.'));
        if malformed {
            return Err(TrunkNameError::Malformed(name.to_string()));
        }
        Ok(Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The fully qualified ref, which is what `Repository.ref` is asked for
    /// so a tag of the same name can never answer in the branch's place.
    pub fn qualified(&self) -> String {
        format!("refs/heads/{}", self.0)
    }
}

impl FromStr for TrunkName {
    type Err = TrunkNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for TrunkName {
    type Error = TrunkNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<TrunkName> for String {
    fn from(name: TrunkName) -> Self {
        name.0
    }
}

impl fmt::Display for TrunkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for TrunkName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_parse_unchanged() {
        for raw in ["main", "master", "release/1.2", "feat_x-y", "v2.0"] {
            assert_eq!(TrunkName::parse(raw).expect(raw).as_str(), raw);
        }
    }

    /// What a person pastes: padding and a fully qualified ref.
    #[test]
    fn surrounding_whitespace_and_the_heads_prefix_are_dropped() {
        assert_eq!(TrunkName::parse("  main\n").expect("trim").as_str(), "main");
        assert_eq!(
            TrunkName::parse("refs/heads/staging")
                .expect("prefix")
                .as_str(),
            "staging"
        );
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(TrunkName::parse(""), Err(TrunkNameError::Empty));
        assert_eq!(TrunkName::parse("   "), Err(TrunkNameError::Empty));
        assert_eq!(TrunkName::parse("refs/heads/"), Err(TrunkNameError::Empty));
    }

    #[test]
    fn inner_whitespace_is_rejected() {
        assert!(matches!(
            TrunkName::parse("my branch"),
            Err(TrunkNameError::Whitespace(_))
        ));
        assert!(matches!(
            TrunkName::parse("a\tb"),
            Err(TrunkNameError::Whitespace(_))
        ));
    }

    #[test]
    fn characters_git_forbids_are_rejected() {
        for raw in ["a~1", "a^", "a:b", "a?", "a*", "a[b", "a\\b"] {
            assert!(
                matches!(
                    TrunkName::parse(raw),
                    Err(TrunkNameError::ForbiddenCharacter(_))
                ),
                "{raw}"
            );
        }
    }

    #[test]
    fn malformed_shapes_are_rejected() {
        for raw in [
            "-main",
            "/main",
            "main/",
            "main.",
            "main.lock",
            "a..b",
            "a//b",
            "a@{1}",
            "@",
            ".hidden",
            "a/.b",
        ] {
            assert!(
                matches!(TrunkName::parse(raw), Err(TrunkNameError::Malformed(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn the_qualified_form_names_the_branch_not_a_tag() {
        let name = TrunkName::parse("main").expect("valid");
        assert_eq!(name.qualified(), "refs/heads/main");
    }

    /// The config file holds plain strings; a bad one must fail to decode
    /// rather than smuggle an invalid name past the constructor.
    #[test]
    fn serde_round_trips_as_a_plain_string_and_validates() {
        let name = TrunkName::parse("staging").expect("valid");
        let json = serde_json::to_string(&name).expect("serialise");
        assert_eq!(json, "\"staging\"");
        let back: TrunkName = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, name);
        assert!(serde_json::from_str::<TrunkName>("\"bad name\"").is_err());
    }
}
