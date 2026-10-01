//! GitHub's Stacks REST API, read-only.
//!
//! `GET /repos/{owner}/{repo}/stacks` lists a repository's stacks, newest
//! first, each with its base ref and its pull requests bottom to top. It is
//! the source of truth for which pull requests form a stack, and it needs no
//! clone, so the feed can group stacks for every watched repository. A 404
//! means stacked pull requests are not enabled for the repository, which is
//! an answer rather than an error.
//!
//! The wire shape is the one `gh stack` (github/gh-stack v0.1.0) decodes.
//! Rostrum never writes through this API: creating, merging and unstacking go
//! through `gh stack`, which owns those flows.

use rostrum_core::{PrNumber, RefName, RepoId, Stack, StackMembers, StackNumber};
use serde::Deserialize;

use crate::error::GitHubError;

/// What a repository said about its stacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoStacks {
    /// The open stacks, in the order GitHub listed them.
    Available(Vec<Stack>),
    /// Stacked pull requests are not enabled here (the API answered 404).
    Unavailable,
}

#[derive(Debug, Deserialize)]
struct StackWire {
    number: u32,
    base: BaseWire,
    /// Absent in some fixtures; a stack with no `open` flag is treated as
    /// open, because hiding a live stack is worse than showing a finished
    /// one for a poll.
    #[serde(default = "open_by_default")]
    open: bool,
    #[serde(default)]
    pull_requests: Vec<StackPrWire>,
}

fn open_by_default() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct BaseWire {
    #[serde(rename = "ref")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct StackPrWire {
    number: u32,
}

/// Decode a page of `GET /repos/{owner}/{repo}/stacks` into stacks for
/// `repo`.
///
/// Closed stacks are dropped: every member has merged or left, and there is
/// nothing left to group. A stack that cannot be represented — a zero number,
/// no pull requests, a pull request listed twice, an unusable base name — is
/// skipped with a warning rather than failing the page, because one odd
/// stack must not ungroup every other one.
pub fn parse_stacks(repo: &RepoId, body: &str) -> Result<Vec<Stack>, GitHubError> {
    let wires: Vec<StackWire> =
        serde_json::from_str(body).map_err(|source| GitHubError::Decode {
            context: format!("stacks for {repo}"),
            source,
        })?;

    let mut stacks = Vec::with_capacity(wires.len());
    for wire in wires.into_iter().filter(|wire| wire.open) {
        let number = wire.number;
        match into_stack(repo, wire) {
            Ok(stack) => stacks.push(stack),
            Err(reason) => {
                tracing::warn!(%repo, number, %reason, "skipping a stack that cannot be represented");
            }
        }
    }
    Ok(stacks)
}

fn into_stack(repo: &RepoId, wire: StackWire) -> Result<Stack, String> {
    let number = StackNumber::new(wire.number).ok_or("stack number zero")?;
    let trunk = RefName::new(wire.base.name).map_err(|err| err.to_string())?;
    let members = StackMembers::new(
        wire.pull_requests
            .into_iter()
            .map(|pr| PrNumber(pr.number))
            .collect(),
    )
    .map_err(|err| err.to_string())?;
    Ok(Stack {
        repo: repo.clone(),
        number: Some(number),
        trunk,
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoId {
        RepoId::new("o", "r")
    }

    /// The shape gh-stack's own decoder test pins (`TestRemoteStack_UnmarshalJSON`).
    const PAGE: &str = r#"[
      {
        "id": 6154,
        "number": 360,
        "node_id": "S_kwABCD",
        "url": "https://api.github.com/repos/o/r/stacks/360",
        "base": {"ref": "main", "sha": "basesha"},
        "open": true,
        "created_at": "2026-01-01T00:00:00Z",
        "pull_requests": [
          {"number": 12, "state": "open", "draft": true, "merged_at": null, "head": {"ref": "feat-1", "sha": "sha1"}},
          {"number": 15, "state": "closed", "draft": false, "merged_at": "2026-01-02T00:00:00Z", "head": {"ref": "feat-2", "sha": "sha2"}}
        ]
      },
      {
        "id": 6100,
        "number": 359,
        "base": {"ref": "develop"},
        "open": false,
        "pull_requests": [{"number": 3}]
      }
    ]"#;

    #[test]
    fn decodes_open_stacks_bottom_first() {
        let stacks = parse_stacks(&repo(), PAGE).expect("decodes");
        assert_eq!(stacks.len(), 1, "the closed stack is dropped");
        let stack = &stacks[0];
        assert_eq!(stack.repo, repo());
        assert_eq!(stack.number.map(StackNumber::get), Some(360));
        assert_eq!(stack.trunk.as_str(), "main");
        assert_eq!(
            stack.members.as_slice(),
            &[PrNumber(12), PrNumber(15)],
            "merged members stay in the stack's order"
        );
    }

    #[test]
    fn an_empty_list_is_no_stacks() {
        assert_eq!(parse_stacks(&repo(), "[]").expect("decodes"), vec![]);
    }

    #[test]
    fn a_stack_without_an_open_flag_counts_as_open() {
        let body = r#"[{"number": 2, "base": {"ref": "main"}, "pull_requests": [{"number": 1}]}]"#;
        assert_eq!(parse_stacks(&repo(), body).expect("decodes").len(), 1);
    }

    #[test]
    fn unrepresentable_stacks_are_skipped_not_fatal() {
        let body = r#"[
          {"number": 0, "base": {"ref": "main"}, "pull_requests": [{"number": 1}]},
          {"number": 1, "base": {"ref": "main"}, "pull_requests": []},
          {"number": 2, "base": {"ref": "main"}, "pull_requests": [{"number": 4}, {"number": 4}]},
          {"number": 3, "base": {"ref": "-x"}, "pull_requests": [{"number": 5}]},
          {"number": 4, "base": {"ref": "main"}, "pull_requests": [{"number": 6}, {"number": 7}]}
        ]"#;
        let stacks = parse_stacks(&repo(), body).expect("decodes");
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].number.map(StackNumber::get), Some(4));
    }

    #[test]
    fn a_body_that_is_not_a_list_is_a_decode_error() {
        assert!(matches!(
            parse_stacks(&repo(), r#"{"message": "nope"}"#),
            Err(GitHubError::Decode { .. })
        ));
    }
}
