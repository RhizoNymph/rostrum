//! Every `gh stack` invocation rostrum makes, as a type, and the exact argv
//! each one becomes.
//!
//! Commands are values so the decision "which flags" lives in one tested
//! function, and so a test can assert the argv without running anything.
//! Only the subcommands rostrum needs exist here; `submit`, `push`, `sync`
//! and `rebase` are deliberately absent — rostrum does its own leased push
//! (`rostrum_git::Repo::push_with_lease`) and never lets another tool push
//! on its behalf.

use std::{fmt, time::Duration};

use rostrum_core::{StackMembers, StackNumber};
use rostrum_git::BranchName;

/// How a stack merge lands its pull requests on the trunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const ALL: [Self; 3] = [Self::Merge, Self::Squash, Self::Rebase];

    /// The value `--merge-method` takes.
    pub fn as_flag_value(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Merge",
            Self::Squash => "Squash",
            Self::Rebase => "Rebase",
        }
    }
}

/// One `gh stack` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhStackCommand {
    /// Create (or extend) the stack on GitHub from existing pull requests,
    /// bottom first: `gh stack link --base <trunk> <pr>...`.
    ///
    /// Pull request numbers rather than branch names, because `link` pushes
    /// any *branch* argument before linking, and rostrum pushes nothing it
    /// did not lease itself. `link` also retargets each pull request's base
    /// to the one below it, which is the base change an arrangement needs.
    Link {
        base: BranchName,
        members: StackMembers,
    },
    /// Track existing local branches as a stack in the clone, bottom first:
    /// `gh stack init --base <trunk> -- <branch>...`. Local only; writes
    /// `<git-dir>/gh-stack` and checks the top branch out.
    Init {
        base: BranchName,
        branches: Vec<BranchName>,
    },
    /// The stack of the checked-out branch, as JSON: `gh stack view --json`.
    ViewJson,
    /// GitHub's atomic, all-or-nothing merge of the whole stack:
    /// `gh stack merge <number> --yes --merge-method <method>`.
    Merge {
        stack: StackNumber,
        method: MergeMethod,
    },
    /// Dissolve the stack on GitHub (and locally, if tracked):
    /// `gh stack unstack <number>`. The pull requests stay open.
    Unstack { stack: StackNumber },
}

impl GhStackCommand {
    /// The arguments after `gh`.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec!["stack".to_string()];
        match self {
            Self::Link { base, members } => {
                argv.extend(["link".into(), "--base".into(), base.to_string()]);
                argv.extend(members.as_slice().iter().map(|n| n.0.to_string()));
            }
            Self::Init { base, branches } => {
                argv.extend(["init".into(), "--base".into(), base.to_string()]);
                // `--` so a branch can never be read as a flag, even though
                // `BranchName` already rejects a leading dash.
                argv.push("--".into());
                argv.extend(branches.iter().map(BranchName::to_string));
            }
            Self::ViewJson => argv.extend(["view".into(), "--json".into()]),
            Self::Merge { stack, method } => {
                argv.extend([
                    "merge".into(),
                    stack.to_string(),
                    // Without it a terminal-less run still merges, but with
                    // the user's *last-used* method; naming it is explicit.
                    "--yes".into(),
                    "--merge-method".into(),
                    method.as_flag_value().into(),
                ]);
            }
            Self::Unstack { stack } => argv.extend(["unstack".into(), stack.to_string()]),
        }
        argv
    }

    /// The subcommand, for logs and errors.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Link { .. } => "stack link",
            Self::Init { .. } => "stack init",
            Self::ViewJson => "stack view",
            Self::Merge { .. } => "stack merge",
            Self::Unstack { .. } => "stack unstack",
        }
    }

    /// How long the call may take before it is killed. Generous: these exist
    /// to turn a hang into an error. A merge waits on GitHub working through
    /// every member, so it gets the most.
    pub fn timeout(&self) -> Duration {
        match self {
            Self::ViewJson | Self::Init { .. } => Duration::from_secs(60),
            Self::Link { .. } | Self::Unstack { .. } => Duration::from_secs(180),
            Self::Merge { .. } => Duration::from_secs(900),
        }
    }

    /// Whether the call needs a git repository as its working directory.
    /// `merge <number>` talks to GitHub alone; the rest read or write the
    /// clone.
    pub fn needs_clone(&self) -> bool {
        !matches!(self, Self::Merge { .. })
    }
}

impl fmt::Display for GhStackCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gh {}", self.argv().join(" "))
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::PrNumber;

    use super::*;

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    fn members(numbers: &[u32]) -> StackMembers {
        StackMembers::new(numbers.iter().copied().map(PrNumber).collect()).expect("valid")
    }

    fn number(n: u32) -> StackNumber {
        StackNumber::new(n).expect("non-zero")
    }

    #[test]
    fn link_names_the_trunk_and_the_pull_requests_bottom_first() {
        let command = GhStackCommand::Link {
            base: branch("main"),
            members: members(&[41, 42, 43]),
        };
        assert_eq!(
            command.argv(),
            vec!["stack", "link", "--base", "main", "41", "42", "43"]
        );
        // Numbers only: a branch argument would make `link` push it.
        assert!(command.argv()[4..].iter().all(|a| a.parse::<u32>().is_ok()));
        assert!(command.needs_clone());
    }

    #[test]
    fn init_adopts_branches_after_an_end_of_options_marker() {
        let command = GhStackCommand::Init {
            base: branch("develop"),
            branches: vec![branch("feat/auth"), branch("feat/api")],
        };
        assert_eq!(
            command.argv(),
            vec![
                "stack",
                "init",
                "--base",
                "develop",
                "--",
                "feat/auth",
                "feat/api"
            ]
        );
    }

    #[test]
    fn view_asks_for_json() {
        assert_eq!(
            GhStackCommand::ViewJson.argv(),
            vec!["stack", "view", "--json"]
        );
    }

    #[test]
    fn merge_targets_the_stack_by_number_never_prompts_and_names_the_method() {
        for (method, flag) in [
            (MergeMethod::Merge, "merge"),
            (MergeMethod::Squash, "squash"),
            (MergeMethod::Rebase, "rebase"),
        ] {
            let command = GhStackCommand::Merge {
                stack: number(7),
                method,
            };
            assert_eq!(
                command.argv(),
                vec!["stack", "merge", "7", "--yes", "--merge-method", flag]
            );
            assert!(!command.needs_clone());
        }
    }

    #[test]
    fn unstack_targets_the_stack_by_number() {
        let command = GhStackCommand::Unstack { stack: number(12) };
        assert_eq!(command.argv(), vec!["stack", "unstack", "12"]);
        assert!(!command.argv().contains(&"--local".to_string()));
    }

    #[test]
    fn no_command_ever_pushes_through_gh() {
        let commands = [
            GhStackCommand::Link {
                base: branch("main"),
                members: members(&[1, 2]),
            },
            GhStackCommand::Init {
                base: branch("main"),
                branches: vec![branch("a")],
            },
            GhStackCommand::ViewJson,
            GhStackCommand::Merge {
                stack: number(1),
                method: MergeMethod::Squash,
            },
            GhStackCommand::Unstack { stack: number(1) },
        ];
        for command in commands {
            let sub = &command.argv()[1];
            assert!(
                !["submit", "push", "sync", "rebase"].contains(&sub.as_str()),
                "{command}"
            );
            assert!(command.timeout() >= Duration::from_secs(60));
        }
    }

    #[test]
    fn display_is_the_command_line() {
        assert_eq!(
            GhStackCommand::Unstack { stack: number(3) }.to_string(),
            "gh stack unstack 3"
        );
    }
}
