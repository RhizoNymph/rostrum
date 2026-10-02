//! What `gh stack` stores locally, read rather than duplicated.
//!
//! gh-stack keeps every locally tracked stack of a repository in one JSON
//! file, `gh-stack`, in the git directory of the worktree it ran in (what
//! `git rev-parse --git-dir` names: `<clone>/.git/gh-stack` for a clone's
//! main worktree). There is no git config or ref involved; a `gh-stack.lock`
//! beside it serialises writers. Schema version 1:
//!
//! ```json
//! { "schemaVersion": 1, "repository": "github.com:owner/repo",
//!   "stacks": [ { "id": "...", "number": 7,
//!                 "trunk": { "branch": "main", "head": "<sha>" },
//!                 "branches": [ { "branch": "feat/a", "base": "<sha>",
//!                                 "pullRequest": { "number": 41, "merged": false } } ] } ] }
//! ```
//!
//! Rostrum only reads it — to know whether a stack is already tracked in the
//! clone before asking `gh stack init` to track it again — and never writes
//! it. GitHub's Stacks API, not this file, is what the feed displays.

use std::path::{Path, PathBuf};

use rostrum_core::{PrNumber, StackNumber};
use serde::Deserialize;

use crate::error::StackOpError;

/// The file name gh-stack uses inside a git directory.
pub const STACK_FILE: &str = "gh-stack";

/// The newest schema this reader understands. A newer file is reported as
/// unknown rather than misread.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileWire {
    schema_version: u32,
    #[serde(default)]
    stacks: Vec<StackWire>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackWire {
    #[serde(default)]
    number: u32,
    trunk: BranchWire,
    #[serde(default)]
    branches: Vec<BranchWire>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchWire {
    branch: String,
    #[serde(default)]
    pull_request: Option<PrWire>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct PrWire {
    number: u32,
    #[serde(default)]
    merged: bool,
}

/// One stack tracked in a clone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalStack {
    /// `None` until gh-stack has learned GitHub's number for it.
    pub number: Option<StackNumber>,
    pub trunk: String,
    pub branches: Vec<LocalBranch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBranch {
    pub name: String,
    pub pr: Option<PrNumber>,
    pub merged: bool,
}

impl LocalStack {
    pub fn contains_branch(&self, name: &str) -> bool {
        self.branches.iter().any(|b| b.name == name)
    }
}

/// What a clone's stack file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalStacks {
    /// No file: nothing has ever been tracked in this git directory.
    Absent,
    Tracked(Vec<LocalStack>),
    /// A schema newer than this reader; its contents are not guessed at.
    NewerSchema(u32),
}

impl LocalStacks {
    pub fn path(git_dir: &Path) -> PathBuf {
        git_dir.join(STACK_FILE)
    }

    /// Read `<git_dir>/gh-stack`.
    pub fn load(git_dir: &Path) -> Result<Self, StackOpError> {
        let path = Self::path(git_dir);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|source| StackOpError::LocalFileDecode {
                path: path.clone(),
                source,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Absent),
            Err(source) => Err(StackOpError::LocalFile { path, source }),
        }
    }

    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        // Read the version first, so a future schema that also changes the
        // shape of `stacks` is reported as such instead of failing to decode.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Version {
            schema_version: u32,
        }
        let version: Version = serde_json::from_str(text)?;
        if version.schema_version > SCHEMA_VERSION {
            return Ok(Self::NewerSchema(version.schema_version));
        }
        let file: FileWire = serde_json::from_str(text)?;
        debug_assert!(file.schema_version <= SCHEMA_VERSION);
        Ok(Self::Tracked(
            file.stacks
                .into_iter()
                .map(|stack| LocalStack {
                    number: StackNumber::new(stack.number),
                    trunk: stack.trunk.branch,
                    branches: stack
                        .branches
                        .into_iter()
                        .map(|branch| LocalBranch {
                            pr: branch.pull_request.as_ref().map(|pr| PrNumber(pr.number)),
                            merged: branch.pull_request.is_some_and(|pr| pr.merged),
                            name: branch.branch,
                        })
                        .collect(),
                })
                .collect(),
        ))
    }

    /// The tracked stack containing `branch`, if any.
    pub fn stack_with_branch(&self, branch: &str) -> Option<&LocalStack> {
        match self {
            Self::Tracked(stacks) => stacks.iter().find(|s| s.contains_branch(branch)),
            Self::Absent | Self::NewerSchema(_) => None,
        }
    }

    /// The tracked stack GitHub numbers `number`, if any.
    pub fn stack_numbered(&self, number: StackNumber) -> Option<&LocalStack> {
        match self {
            Self::Tracked(stacks) => stacks.iter().find(|s| s.number == Some(number)),
            Self::Absent | Self::NewerSchema(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped after gh-stack's `internal/stack/schema.json`.
    const FILE: &str = r#"{
  "schemaVersion": 1,
  "repository": "github.com:o/r",
  "stacks": [
    {
      "id": "S_kwABCD",
      "number": 7,
      "trunk": {"branch": "main", "head": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
      "branches": [
        {"branch": "feat/a", "base": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
         "pullRequest": {"number": 41, "id": "PR_1", "url": "https://github.com/o/r/pull/41", "merged": true}},
        {"branch": "feat/b", "pullRequest": {"number": 42}},
        {"branch": "feat/c"}
      ]
    },
    {
      "trunk": {"branch": "develop"},
      "branches": [{"branch": "x"}]
    }
  ]
}"#;

    #[test]
    fn decodes_the_schema_one_file() {
        let LocalStacks::Tracked(stacks) = LocalStacks::parse(FILE).expect("decodes") else {
            panic!("expected tracked stacks");
        };
        assert_eq!(stacks.len(), 2);
        assert_eq!(stacks[0].number, StackNumber::new(7));
        assert_eq!(stacks[0].trunk, "main");
        assert_eq!(
            stacks[0].branches,
            vec![
                LocalBranch {
                    name: "feat/a".into(),
                    pr: Some(PrNumber(41)),
                    merged: true
                },
                LocalBranch {
                    name: "feat/b".into(),
                    pr: Some(PrNumber(42)),
                    merged: false
                },
                LocalBranch {
                    name: "feat/c".into(),
                    pr: None,
                    merged: false
                },
            ]
        );
        assert_eq!(stacks[1].number, None, "no number yet");
    }

    #[test]
    fn lookups_by_branch_and_number() {
        let file = LocalStacks::parse(FILE).expect("decodes");
        assert_eq!(
            file.stack_with_branch("feat/b").map(|s| s.trunk.as_str()),
            Some("main")
        );
        assert_eq!(
            file.stack_with_branch("main"),
            None,
            "the trunk is not a member"
        );
        assert!(
            file.stack_numbered(StackNumber::new(7).expect("non-zero"))
                .is_some()
        );
        assert!(
            file.stack_numbered(StackNumber::new(8).expect("non-zero"))
                .is_none()
        );
    }

    #[test]
    fn a_newer_schema_is_not_guessed_at() {
        let text = r#"{"schemaVersion": 2, "stacks": "a different shape"}"#;
        assert_eq!(
            LocalStacks::parse(text).expect("reads the version"),
            LocalStacks::NewerSchema(2)
        );
        assert_eq!(LocalStacks::NewerSchema(2).stack_with_branch("x"), None);
    }

    #[test]
    fn a_missing_file_is_absent_and_a_corrupt_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("rostrum-stack-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let _ = std::fs::remove_file(LocalStacks::path(&dir));
        assert_eq!(
            LocalStacks::load(&dir).expect("absent"),
            LocalStacks::Absent
        );

        std::fs::write(LocalStacks::path(&dir), "{ not json").expect("write");
        assert!(matches!(
            LocalStacks::load(&dir),
            Err(StackOpError::LocalFileDecode { .. })
        ));

        std::fs::write(LocalStacks::path(&dir), FILE).expect("write");
        assert!(matches!(
            LocalStacks::load(&dir),
            Ok(LocalStacks::Tracked(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
