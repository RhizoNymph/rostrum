//! `gh stack view --json`: the stack of the checked-out branch, as gh-stack
//! tracks it locally.
//!
//! Rostrum reads it once, right after `gh stack init`, to confirm the clone
//! now tracks exactly the branches it asked for. The shape is gh-stack
//! v0.1.0's `viewJSONOutput`.

use rostrum_core::PrNumber;
use serde::Deserialize;

use crate::error::StackOpError;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackView {
    pub trunk: String,
    #[serde(default)]
    pub current_branch: String,
    #[serde(default)]
    pub branches: Vec<ViewBranch>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBranch {
    pub name: String,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub is_merged: bool,
    #[serde(default)]
    pub is_queued: bool,
    #[serde(default)]
    pub needs_rebase: bool,
    #[serde(default)]
    pub pr: Option<ViewPr>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ViewPr {
    pub number: u32,
    #[serde(default)]
    pub url: Option<String>,
    /// `OPEN`, `MERGED` or `QUEUED`.
    pub state: String,
}

impl StackView {
    pub fn parse(stdout: &str) -> Result<Self, StackOpError> {
        serde_json::from_str(stdout).map_err(|source| StackOpError::ViewDecode { source })
    }

    pub fn branch_names(&self) -> Vec<&str> {
        self.branches.iter().map(|b| b.name.as_str()).collect()
    }

    /// The pull request numbers gh-stack associated with the branches, in
    /// stack order; `None` for a branch it found no pull request for.
    pub fn pr_numbers(&self) -> Vec<Option<PrNumber>> {
        self.branches
            .iter()
            .map(|b| b.pr.as_ref().map(|pr| PrNumber(pr.number)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `gh stack view --json` prints for a three-branch stack with the
    /// top checked out, the bottom merged, and one branch needing a rebase.
    const FIXTURE: &str = r#"{
  "trunk": "main",
  "currentBranch": "feat/ui",
  "branches": [
    {
      "name": "feat/auth",
      "head": "1111111111111111111111111111111111111111",
      "base": "0000000000000000000000000000000000000aaa",
      "isCurrent": false,
      "isMerged": true,
      "isQueued": false,
      "needsRebase": false,
      "pr": {"number": 41, "url": "https://github.com/o/r/pull/41", "state": "MERGED"}
    },
    {
      "name": "feat/api",
      "base": "1111111111111111111111111111111111111111",
      "isCurrent": false,
      "isMerged": false,
      "isQueued": false,
      "needsRebase": true,
      "pr": {"number": 42, "state": "OPEN"}
    },
    {
      "name": "feat/ui",
      "isCurrent": true,
      "isMerged": false,
      "isQueued": false,
      "needsRebase": false
    }
  ]
}"#;

    #[test]
    fn decodes_every_field_gh_stack_writes() {
        let view = StackView::parse(FIXTURE).expect("decodes");
        assert_eq!(view.trunk, "main");
        assert_eq!(view.current_branch, "feat/ui");
        assert_eq!(
            view.branch_names(),
            vec!["feat/auth", "feat/api", "feat/ui"]
        );
        assert_eq!(
            view.pr_numbers(),
            vec![Some(PrNumber(41)), Some(PrNumber(42)), None]
        );
        assert!(view.branches[0].is_merged);
        assert_eq!(
            view.branches[0].pr.as_ref().map(|p| p.state.as_str()),
            Some("MERGED")
        );
        assert!(view.branches[1].needs_rebase);
        assert_eq!(view.branches[1].head, None, "head is omitted when empty");
        assert!(view.branches[2].is_current);
    }

    #[test]
    fn a_stack_with_no_branches_decodes() {
        let view = StackView::parse(r#"{"trunk":"main","currentBranch":"main","branches":[]}"#)
            .expect("decodes");
        assert!(view.branches.is_empty());
    }

    #[test]
    fn gh_s_human_error_is_not_json() {
        assert!(matches!(
            StackView::parse("✗ current branch \"main\" is not part of a stack"),
            Err(StackOpError::ViewDecode { .. })
        ));
    }
}
