//! Running `gh`: the only place in rostrum that does.
//!
//! The discipline is rostrum-git's (`command.rs`): every call is bounded by a
//! timeout and killed on expiry, stdin is closed so nothing can stop to ask,
//! and the outcome is a plain value. The environment differs on purpose,
//! the way rostrum-handoff's tmux does: `gh` authenticates with the user's
//! own setup — `GH_TOKEN`, `GH_CONFIG_DIR`, the keyring through `DBUS_*`,
//! `HOME` — so it inherits the environment, minus the few variables that
//! would change *which* repository answers:
//!
//! - `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR` hijack
//!   every `git` that `gh stack` runs inside, exactly as they would rostrum's.
//! - `GH_REPO` is **set**, to the repository the action is for, so a clone
//!   with a fork as its first remote cannot redirect a merge to the fork.
//! - `GH_HOST` is left alone: an Enterprise user sets it deliberately.
//!
//! Prompts are switched off (`GH_PROMPT_DISABLED`, `GIT_TERMINAL_PROMPT=0`)
//! and colour with them, since stdout is parsed or shown verbatim.
//!
//! [`GhRunner`] is the seam tests use: [`GhCli`] runs the real binary, and a
//! test double records the [`GhStackCommand`]s it is handed.

use std::{future::Future, path::Path, process::Stdio, time::Instant};

use rostrum_core::RepoId;
use tokio::process::Command;

use super::argv::GhStackCommand;
use crate::error::StackOpError;

/// What a finished `gh` call produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl GhOutput {
    /// stdout and stderr together, trimmed, for showing to the user.
    pub fn message(&self) -> String {
        [self.stdout.trim(), self.stderr.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Fail unless `gh` exited zero, recognising a missing extension.
    pub fn require_success(self, command: &GhStackCommand) -> Result<Self, StackOpError> {
        if self.success {
            return Ok(self);
        }
        if is_missing_extension(&self.stderr) {
            return Err(StackOpError::GhStackMissing);
        }
        Err(StackOpError::GhFailed {
            command: command.to_string(),
            code: self.code,
            message: self.message(),
        })
    }
}

/// `gh` answers an unknown subcommand with `unknown command "stack" for "gh"`.
fn is_missing_extension(stderr: &str) -> bool {
    stderr.contains("unknown command \"stack\"")
}

/// Something that can run a `gh stack` command for a repository from a
/// working directory.
pub trait GhRunner: Send + Sync {
    fn run(
        &self,
        cwd: &Path,
        repo: &RepoId,
        command: &GhStackCommand,
    ) -> impl Future<Output = Result<GhOutput, StackOpError>> + Send;
}

/// Variables removed from the inherited environment. Each one changes which
/// repository a `git` inside `gh stack` reads.
pub const ENV_REMOVED: [&str; 5] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    // Set explicitly below; never inherited.
    "GH_REPO",
];

/// Variables forced on every call.
pub const ENV_FORCED: [(&str, &str); 6] = [
    ("GH_PROMPT_DISABLED", "1"),
    ("GH_NO_UPDATE_NOTIFIER", "1"),
    ("GH_SPINNER_DISABLED", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("NO_COLOR", "1"),
    ("CLICOLOR", "0"),
];

/// The real `gh`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GhCli;

impl GhRunner for GhCli {
    async fn run(
        &self,
        cwd: &Path,
        repo: &RepoId,
        command: &GhStackCommand,
    ) -> Result<GhOutput, StackOpError> {
        let argv = command.argv();
        let mut child = Command::new("gh");
        child.args(&argv).current_dir(cwd);
        for name in ENV_REMOVED {
            child.env_remove(name);
        }
        for (name, value) in ENV_FORCED {
            child.env(name, value);
        }
        child.env("GH_REPO", repo.to_string());
        child
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let started = Instant::now();
        let spawned = child
            .spawn()
            .map_err(|source| StackOpError::GhSpawn { source })?;
        let budget = command.timeout();
        let output = match tokio::time::timeout(budget, spawned.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(source)) => return Err(StackOpError::GhSpawn { source }),
            Err(_) => {
                return Err(StackOpError::GhTimeout {
                    command: command.to_string(),
                    after: budget,
                });
            }
        };
        let out = GhOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        tracing::info!(
            command = command.name(),
            %repo,
            cwd = %cwd.display(),
            code = ?out.code,
            elapsed_ms = started.elapsed().as_millis(),
            "ran gh"
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variables_that_redirect_git_are_removed() {
        for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GH_REPO"] {
            assert!(ENV_REMOVED.contains(&name), "{name}");
        }
        // Authentication must survive.
        for name in ["GH_TOKEN", "GH_CONFIG_DIR", "HOME", "PATH", "GH_HOST"] {
            assert!(!ENV_REMOVED.contains(&name), "{name}");
        }
    }

    #[test]
    fn prompts_are_disabled() {
        let forced: std::collections::HashMap<_, _> = ENV_FORCED.into_iter().collect();
        assert_eq!(forced.get("GH_PROMPT_DISABLED"), Some(&"1"));
        assert_eq!(forced.get("GIT_TERMINAL_PROMPT"), Some(&"0"));
    }

    fn output(success: bool, stdout: &str, stderr: &str) -> GhOutput {
        GhOutput {
            success,
            code: Some(if success { 0 } else { 1 }),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn a_failure_carries_gh_s_own_words() {
        let err = output(
            false,
            "",
            "✗ cannot merge the whole stack: pull request #4 is a draft\n",
        )
        .require_success(&GhStackCommand::ViewJson)
        .expect_err("failed");
        let StackOpError::GhFailed {
            message, command, ..
        } = err
        else {
            panic!("expected GhFailed, got {err:?}");
        };
        assert_eq!(
            message,
            "✗ cannot merge the whole stack: pull request #4 is a draft"
        );
        assert_eq!(command, "gh stack view --json");
    }

    #[test]
    fn a_missing_extension_is_named() {
        let err = output(false, "", "unknown command \"stack\" for \"gh\"\n")
            .require_success(&GhStackCommand::ViewJson)
            .expect_err("failed");
        assert!(matches!(err, StackOpError::GhStackMissing));
    }

    #[test]
    fn the_message_joins_both_streams() {
        assert_eq!(output(true, "a\n", "b\n").message(), "a\nb");
        assert_eq!(output(true, " \n", "b\n").message(), "b");
    }
}
