//! `rostrum://` links that open one pull request or one issue.
//!
//! Other apps (Orchestral's "Open in Rostrum") hand these to the desktop app
//! as a launch argument and to the phone as an intent:
//!
//! ```text
//! rostrum://pr?repo=<owner>/<name>&number=<n>
//! rostrum://issue?repo=<owner>/<name>&number=<n>
//! ```
//!
//! `repo` may be percent-encoded (`owner%2Fname`); unknown parameters and a
//! `#fragment` are ignored. Everything else is checked here, once, so a
//! [`RostrumLink`] in hand always names a well-formed repository and a
//! positive number. The repository is checked against GitHub's own name rules
//! rather than [`RepoId`]'s lenient parser, because it arrives from outside
//! and is spliced into API paths. The Android app parses the same grammar in
//! Kotlin (`ItemLinks.kt`); the two are tested against the same cases.

use std::fmt;

use crate::{IssueNumber, PrNumber, RepoId, Selection};

/// The longest link accepted. Real links are well under 200 bytes.
pub const MAX_LINK_LEN: usize = 2048;

/// GitHub's limits: a login is at most 39 characters, a repository name at
/// most 100.
const MAX_OWNER_LEN: usize = 39;
const MAX_NAME_LEN: usize = 100;

/// One pull request or one issue to open.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RostrumLink {
    PullRequest { repo: RepoId, number: PrNumber },
    Issue { repo: RepoId, number: IssueNumber },
}

/// Why a string is not a link rostrum can open. Each reads as the end of
/// "Rostrum can't open this link: …".
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("it is longer than {MAX_LINK_LEN} characters")]
    TooLong,
    #[error("it is not a rostrum:// link")]
    NotRostrum,
    #[error(
        "rostrum://{0} is not something rostrum opens (expected rostrum://pr or rostrum://issue)"
    )]
    UnknownTarget(String),
    #[error("it has no `{0}` parameter")]
    Missing(&'static str),
    #[error("it gives `{0}` more than once")]
    Repeated(&'static str),
    #[error("its `{0}` parameter is not valid percent-encoding")]
    BadEncoding(&'static str),
    #[error("`{0}` is not a GitHub repository (expected owner/name)")]
    InvalidRepo(String),
    #[error("`{0}` is not a pull request or issue number")]
    InvalidNumber(String),
}

/// Which of the two link kinds a host names.
#[derive(Clone, Copy)]
enum Kind {
    PullRequest,
    Issue,
}

impl RostrumLink {
    /// Parse and validate a `rostrum://pr` or `rostrum://issue` link.
    pub fn parse(input: &str) -> Result<Self, LinkError> {
        let input = input.trim();
        if input.len() > MAX_LINK_LEN {
            return Err(LinkError::TooLong);
        }
        let rest = strip_prefix_ignore_case(input, "rostrum://").ok_or(LinkError::NotRostrum)?;
        // The fragment is never part of the request.
        let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
        let (target, query) = rest.split_once('?').unwrap_or((rest, ""));
        // `rostrum://pr/?…` is the same link as `rostrum://pr?…`.
        let host = target.strip_suffix('/').unwrap_or(target);
        let kind = if host.eq_ignore_ascii_case("pr") {
            Kind::PullRequest
        } else if host.eq_ignore_ascii_case("issue") {
            Kind::Issue
        } else {
            return Err(LinkError::UnknownTarget(host.to_string()));
        };

        let mut repo = None;
        let mut number = None;
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            // A key that does not decode cannot be one of ours.
            let Some(key) = percent_decode(key) else {
                continue;
            };
            let (slot, name) = match key.as_str() {
                "repo" => (&mut repo, "repo"),
                "number" => (&mut number, "number"),
                _ => continue,
            };
            if slot.is_some() {
                return Err(LinkError::Repeated(name));
            }
            *slot = Some(percent_decode(value).ok_or(LinkError::BadEncoding(name))?);
        }

        let repo = parse_repo(&repo.ok_or(LinkError::Missing("repo"))?)?;
        let number = parse_number(&number.ok_or(LinkError::Missing("number"))?)?;
        Ok(match kind {
            Kind::PullRequest => Self::PullRequest {
                repo,
                number: PrNumber(number),
            },
            Kind::Issue => Self::Issue {
                repo,
                number: IssueNumber(number),
            },
        })
    }

    pub fn repo(&self) -> &RepoId {
        match self {
            Self::PullRequest { repo, .. } | Self::Issue { repo, .. } => repo,
        }
    }

    /// What the detail pane should show.
    pub fn selection(&self) -> Selection {
        match self {
            Self::PullRequest { repo, number } => Selection::PullRequest {
                repo: repo.clone(),
                number: *number,
            },
            Self::Issue { repo, number } => Selection::Issue {
                repo: repo.clone(),
                number: *number,
            },
        }
    }
}

/// The canonical form: lower-case host, `repo` with its slash encoded.
impl fmt::Display for RostrumLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (host, number) = match self {
            Self::PullRequest { number, .. } => ("pr", number.0),
            Self::Issue { number, .. } => ("issue", number.0),
        };
        let repo = self.repo();
        write!(
            f,
            "rostrum://{host}?repo={}%2F{}&number={number}",
            repo.owner(),
            repo.name()
        )
    }
}

fn strip_prefix_ignore_case<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let head = input.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &input[prefix.len()..])
}

/// Decode `%XX` escapes; `None` for a truncated or non-hex escape, or bytes
/// that are not UTF-8. `+` is left alone: no valid value contains one.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `owner/name` under GitHub's rules: an owner of letters, digits and inner
/// single hyphens; a name of letters, digits, `.`, `_` and `-`, never `.` or
/// `..`.
fn parse_repo(value: &str) -> Result<RepoId, LinkError> {
    let invalid = || LinkError::InvalidRepo(value.to_string());
    let (owner, name) = value.split_once('/').ok_or_else(invalid)?;
    let owner_ok = (1..=MAX_OWNER_LEN).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && !owner.contains("--");
    let name_ok = (1..=MAX_NAME_LEN).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && name != "."
        && name != "..";
    if owner_ok && name_ok {
        Ok(RepoId::new(owner, name))
    } else {
        Err(invalid())
    }
}

/// A positive decimal number that fits in 32 bits; digits only.
fn parse_number(value: &str) -> Result<u32, LinkError> {
    let invalid = || LinkError::InvalidNumber(value.to_string());
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    match value.parse::<u32>() {
        Ok(0) | Err(_) => Err(invalid()),
        Ok(number) => Ok(number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(repo: &str, number: u32) -> RostrumLink {
        RostrumLink::PullRequest {
            repo: repo.parse().expect("repo"),
            number: PrNumber(number),
        }
    }

    fn issue(repo: &str, number: u32) -> RostrumLink {
        RostrumLink::Issue {
            repo: repo.parse().expect("repo"),
            number: IssueNumber(number),
        }
    }

    #[test]
    fn a_pull_request_link_with_an_encoded_repo() {
        assert_eq!(
            RostrumLink::parse("rostrum://pr?repo=octo%2Frepo&number=42"),
            Ok(pr("octo/repo", 42))
        );
    }

    #[test]
    fn an_issue_link_with_a_plain_slash() {
        assert_eq!(
            RostrumLink::parse("rostrum://issue?repo=octo/repo&number=7"),
            Ok(issue("octo/repo", 7))
        );
    }

    #[test]
    fn parameters_come_in_any_order_and_unknown_ones_are_ignored() {
        assert_eq!(
            RostrumLink::parse("rostrum://pr?from=orchestral&number=3&x&repo=a-b/c.d_e-f&y=%ZZ"),
            Ok(pr("a-b/c.d_e-f", 3))
        );
    }

    #[test]
    fn scheme_and_host_ignore_case_and_a_trailing_slash_and_fragment() {
        assert_eq!(
            RostrumLink::parse("  ROSTRUM://PR/?repo=octo%2frepo&number=9#top \n"),
            Ok(pr("octo/repo", 9))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://Issue?repo=octo/repo&number=1"),
            Ok(issue("octo/repo", 1))
        );
    }

    #[test]
    fn every_encoded_character_decodes() {
        // `%6F` is `o`: a fully escaped value is still the same repository.
        assert_eq!(
            RostrumLink::parse("rostrum://pr?r%65po=%6Fcto%2F%72epo&number=%34%32"),
            Ok(pr("octo/repo", 42))
        );
    }

    #[test]
    fn other_schemes_and_targets_are_refused() {
        assert_eq!(
            RostrumLink::parse("https://github.com/octo/repo/pull/1"),
            Err(LinkError::NotRostrum)
        );
        assert_eq!(
            RostrumLink::parse("rostrum:pr?repo=octo/repo&number=1"),
            Err(LinkError::NotRostrum)
        );
        assert_eq!(
            RostrumLink::parse("rostrum://pair?code=ABCD"),
            Err(LinkError::UnknownTarget("pair".into()))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://pr/extra?repo=octo/repo&number=1"),
            Err(LinkError::UnknownTarget("pr/extra".into()))
        );
    }

    #[test]
    fn missing_and_repeated_parameters_are_refused() {
        assert_eq!(
            RostrumLink::parse("rostrum://pr?number=1"),
            Err(LinkError::Missing("repo"))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://issue?repo=octo/repo"),
            Err(LinkError::Missing("number"))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://pr"),
            Err(LinkError::Missing("repo"))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://pr?repo=a/b&repo=c/d&number=1"),
            Err(LinkError::Repeated("repo"))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://pr?repo=a/b&number=1&number=2"),
            Err(LinkError::Repeated("number"))
        );
    }

    #[test]
    fn broken_encoding_in_a_known_parameter_is_refused() {
        assert_eq!(
            RostrumLink::parse("rostrum://pr?repo=octo%2&number=1"),
            Err(LinkError::BadEncoding("repo"))
        );
        assert_eq!(
            RostrumLink::parse("rostrum://pr?repo=octo%zzrepo&number=1"),
            Err(LinkError::BadEncoding("repo"))
        );
        // Escapes that decode to bytes which are not UTF-8.
        assert_eq!(
            RostrumLink::parse("rostrum://pr?repo=a/b&number=%FF"),
            Err(LinkError::BadEncoding("number"))
        );
    }

    #[test]
    fn repositories_outside_githubs_rules_are_refused() {
        for bad in [
            "",
            "octo",
            "octo/",
            "/repo",
            "a/b/c",
            "octo/..",
            "octo/.",
            "-octo/repo",
            "octo-/repo",
            "oc--to/repo",
            "oc_to/repo",
            "octo/re po",
            "octo/re+po",
            "octo/re%2Fpo",
            "https://github.com/octo/repo",
            "octo/repo\u{e9}",
        ] {
            let link = format!(
                "rostrum://pr?repo={}&number=1",
                bad.replace('%', "%25").replace(' ', "%20")
            );
            assert_eq!(
                RostrumLink::parse(&link),
                Err(LinkError::InvalidRepo(bad.to_string())),
                "{link}"
            );
        }
        let long_owner = "o".repeat(MAX_OWNER_LEN + 1);
        assert!(matches!(
            RostrumLink::parse(&format!("rostrum://pr?repo={long_owner}/r&number=1")),
            Err(LinkError::InvalidRepo(_))
        ));
        let longest_owner = "o".repeat(MAX_OWNER_LEN);
        assert!(
            RostrumLink::parse(&format!("rostrum://pr?repo={longest_owner}/r&number=1")).is_ok()
        );
    }

    #[test]
    fn numbers_must_be_positive_plain_decimals_in_range() {
        for bad in [
            "",
            "0",
            "-1",
            "+1",
            "1.0",
            "0x10",
            "4294967296",
            "one",
            " 1",
        ] {
            let link = format!(
                "rostrum://issue?repo=octo/repo&number={}",
                bad.replace(' ', "%20").replace('+', "%2B")
            );
            assert_eq!(
                RostrumLink::parse(&link),
                Err(LinkError::InvalidNumber(bad.to_string())),
                "{link}"
            );
        }
        assert_eq!(
            RostrumLink::parse("rostrum://issue?repo=octo/repo&number=4294967295"),
            Ok(issue("octo/repo", u32::MAX))
        );
    }

    #[test]
    fn overlong_links_are_refused_before_anything_else() {
        let link = format!(
            "rostrum://pr?repo=octo/repo&number=1&pad={}",
            "x".repeat(MAX_LINK_LEN)
        );
        assert_eq!(RostrumLink::parse(&link), Err(LinkError::TooLong));
    }

    #[test]
    fn the_canonical_form_round_trips() {
        for link in [pr("octo/repo", 42), issue("a-b/c.d", 1)] {
            assert_eq!(RostrumLink::parse(&link.to_string()), Ok(link.clone()));
        }
        assert_eq!(
            pr("octo/repo", 42).to_string(),
            "rostrum://pr?repo=octo%2Frepo&number=42"
        );
    }

    #[test]
    fn a_link_selects_its_item_on_its_tab() {
        let selection = pr("octo/repo", 42).selection();
        assert_eq!(
            selection,
            Selection::PullRequest {
                repo: "octo/repo".parse().expect("repo"),
                number: PrNumber(42)
            }
        );
        assert_eq!(selection.tab(), crate::FeedTab::PullRequests);
        assert_eq!(
            issue("octo/repo", 7).selection().tab(),
            crate::FeedTab::Issues
        );
    }
}
