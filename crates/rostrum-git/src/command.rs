//! Running `git` safely.
//!
//! Two things make this more than a wrapper around [`tokio::process::Command`].
//!
//! **The environment is rebuilt, not inherited.** `git -C <path>` does *not*
//! isolate the child: `GIT_DIR` in the environment overrides repository
//! discovery entirely, so `git -C /some/clone status` can cheerfully report on
//! a completely different repository. The same is true of `GIT_WORK_TREE`,
//! `GIT_INDEX_FILE`, and `GIT_CONFIG_*`. The only way to be sure which
//! repository answered is to clear the environment and put back a known
//! allowlist.
//!
//! **Every call is bounded.** A pre-commit hook, an `ssh` waiting on a host key,
//! or a credential helper with no terminal will otherwise hang forever, and in
//! a GUI that means a frozen window with no explanation.

use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

use tokio::process::Command;

use crate::error::GitError;

/// Which timeout budget a command draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    /// Local reads: `status`, `rev-parse`, `rev-list`, `show-ref`.
    Read,
    /// Anything that talks to a remote.
    Network,
    /// Anything that moves HEAD or touches the worktree. The only kind whose
    /// timeout can leave the repository changed.
    Write,
}

/// How long each class of command may take.
///
/// The defaults are generous on purpose: they exist to convert a hang into an
/// error, not to police slowness. A large repository's `status` can take
/// seconds, and a rebase over hundreds of commits with hooks can take minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    pub read: Duration,
    pub network: Duration,
    pub write: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            read: Duration::from_secs(10),
            network: Duration::from_secs(120),
            write: Duration::from_secs(300),
        }
    }
}

impl Timeouts {
    pub fn for_kind(&self, kind: CommandKind) -> Duration {
        match kind {
            CommandKind::Read => self.read,
            CommandKind::Network => self.network,
            CommandKind::Write => self.write,
        }
    }
}

/// Environment variables passed through to git.
///
/// `DISPLAY` and `WAYLAND_DISPLAY` are deliberately **absent**: with either
/// set, git's `SSH_ASKPASS` can open a graphical password dialog from a process
/// the user did not start and cannot see. `SSH_AUTH_SOCK` is deliberately
/// **kept**, because it is how agent-based SSH authentication works and
/// dropping it would break every fetch over `git@` for no security gain — the
/// agent is the user's own, and rostrum only ever reads.
const ENV_ALLOWLIST: [&str; 8] = [
    // Without this, `git` itself cannot be found.
    "PATH",
    // git reads `~/.gitconfig` and `~/.ssh/` through it.
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "SSH_AUTH_SOCK",
];

/// Environment forced on every child.
const ENV_FORCED: [(&str, &str); 9] = [
    // Never block on a username/password prompt.
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    // `false`, not `true`: `false` exits non-zero, so git aborts loudly instead
    // of silently accepting an unreviewed commit message or todo list.
    ("GIT_EDITOR", "false"),
    ("GIT_SEQUENCE_EDITOR", "false"),
    ("GIT_PAGER", "cat"),
    ("SSH_ASKPASS_REQUIRE", "never"),
    // Output is parsed, so the locale must be the one the parsers were written
    // against — and it is what makes the single prose match in `fetch` sound.
    ("LC_ALL", "C"),
    ("LANG", "C"),
    ("TERM", "dumb"),
];

/// What a finished `git` invocation produced.
#[derive(Clone, Debug)]
pub struct Run {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// stdout and stderr together, trimmed — git splits its account of a
    /// conflict across both, and the user wants the whole story.
    pub fn message(&self) -> String {
        let mut message = String::new();
        for part in [self.stdout.trim_end(), self.stderr.trim_end()] {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(part);
        }
        message
    }

    /// Fail unless git exited zero.
    pub fn require_success(self, command: &str) -> Result<Self, GitError> {
        if self.success {
            Ok(self)
        } else {
            Err(GitError::Failed {
                command: command.to_string(),
                code: self.code,
                stderr: if self.stderr.trim().is_empty() {
                    self.stdout.trim().to_string()
                } else {
                    self.stderr.trim().to_string()
                },
            })
        }
    }
}

/// Flags prepended to every invocation.
///
/// `--no-optional-locks` is a **top-level** flag. `git status --no-optional-locks`
/// is rejected outright, so it has to live here rather than with the
/// subcommand. It stops a background read from touching the index and racing
/// whatever the user is doing in their own terminal.
///
/// `-c color.ui=false` survives a user config that forces colour on, which
/// would otherwise wrap every parsed field in escape sequences.
/// `-c submodule.recurse=true` in a user's config would make a rebase or
/// checkout walk into submodules, so it is pinned off.
pub fn common_args(root: &Path) -> Vec<String> {
    vec![
        "-C".to_string(),
        root.display().to_string(),
        "--no-optional-locks".to_string(),
        "--no-pager".to_string(),
        "-c".to_string(),
        "color.ui=false".to_string(),
        "-c".to_string(),
        "submodule.recurse=false".to_string(),
    ]
}

/// Run one `git` invocation to completion, or kill it.
pub async fn run(
    root: &Path,
    args: &[String],
    kind: CommandKind,
    timeouts: &Timeouts,
) -> Result<Run, GitError> {
    let mut argv = common_args(root);
    argv.extend_from_slice(args);

    let mut command = Command::new("git");
    command.args(&argv);

    command.env_clear();
    for name in ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    for (name, value) in ENV_FORCED {
        command.env(name, value);
    }
    // Set separately because it must be present and *empty*: `LANGUAGE` is a
    // list of fallback locales that overrides `LC_ALL` for messages, and
    // unsetting it would let an inherited value through.
    command.env("LANGUAGE", "");

    // Nothing this crate runs has anything to read, and a git that finds a
    // terminal on stdin is a git that can stop and wait for one.
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    // The timeout below drops the child; this is what turns that into a kill.
    command.kill_on_drop(true);

    let started = Instant::now();
    let child = command
        .spawn()
        .map_err(|source| GitError::Spawn { source })?;

    let budget = timeouts.for_kind(kind);
    let output = match tokio::time::timeout(budget, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(source)) => return Err(GitError::Spawn { source }),
        Err(_) => {
            return Err(GitError::Timeout {
                command: args.join(" "),
                after: budget,
                kind,
            });
        }
    };

    let run = Run {
        success: output.status.success(),
        code: output.status.code(),
        // git's output can contain paths that are not valid UTF-8. Nothing here
        // parses a path — only counts and object ids — so a lossy decode cannot
        // change any answer, and it keeps one bad filename from failing a read.
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };

    tracing::debug!(
        args = %args.join(" "),
        code = ?run.code,
        elapsed_ms = started.elapsed().as_millis(),
        "ran git"
    );

    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--no-optional-locks` is only accepted before the subcommand; git
    /// rejects `git status --no-optional-locks` outright.
    #[test]
    fn the_common_prefix_puts_top_level_flags_before_the_subcommand() {
        let args = common_args(Path::new("/repo"));
        assert_eq!(
            args,
            vec![
                "-C",
                "/repo",
                "--no-optional-locks",
                "--no-pager",
                "-c",
                "color.ui=false",
                "-c",
                "submodule.recurse=false",
            ]
        );
    }

    /// A graphical askpass dialog from an invisible process is worse than a
    /// failed fetch; agent auth is how fetches actually succeed.
    #[test]
    fn the_allowlist_drops_display_and_keeps_the_ssh_agent() {
        assert!(ENV_ALLOWLIST.contains(&"SSH_AUTH_SOCK"));
        assert!(!ENV_ALLOWLIST.contains(&"DISPLAY"));
        assert!(!ENV_ALLOWLIST.contains(&"WAYLAND_DISPLAY"));
        // `-C` does not isolate the child from these.
        for hijacker in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
            assert!(!ENV_ALLOWLIST.contains(&hijacker), "{hijacker}");
        }
    }

    /// `true` would let git accept an unreviewed message in silence.
    #[test]
    fn the_editors_are_set_to_something_that_fails() {
        let forced: std::collections::HashMap<_, _> = ENV_FORCED.into_iter().collect();
        assert_eq!(forced.get("GIT_EDITOR"), Some(&"false"));
        assert_eq!(forced.get("GIT_SEQUENCE_EDITOR"), Some(&"false"));
        assert_eq!(forced.get("GIT_TERMINAL_PROMPT"), Some(&"0"));
    }

    #[test]
    fn each_kind_draws_from_its_own_budget() {
        let timeouts = Timeouts::default();
        assert_eq!(
            timeouts.for_kind(CommandKind::Read),
            Duration::from_secs(10)
        );
        assert_eq!(
            timeouts.for_kind(CommandKind::Network),
            Duration::from_secs(120)
        );
        assert_eq!(
            timeouts.for_kind(CommandKind::Write),
            Duration::from_secs(300)
        );
    }

    /// Only a killed *write* can have changed anything.
    #[test]
    fn only_a_timed_out_write_may_have_written() {
        let timeout = |kind| GitError::Timeout {
            command: "rebase".to_string(),
            after: Duration::from_secs(1),
            kind,
        };
        assert!(timeout(CommandKind::Write).may_have_written());
        assert!(!timeout(CommandKind::Read).may_have_written());
        assert!(!timeout(CommandKind::Network).may_have_written());
        assert!(
            !GitError::Failed {
                command: "rebase".to_string(),
                code: Some(1),
                stderr: String::new(),
            }
            .may_have_written(),
            "a refusal leaves the repository as it was"
        );
    }

    /// git splits its account of a conflict across both streams.
    #[test]
    fn a_message_joins_both_streams_and_skips_empty_ones() {
        let run = |stdout: &str, stderr: &str| Run {
            success: true,
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        };
        assert_eq!(
            run("CONFLICT (content): Merge conflict in f.txt\n", "").message(),
            "CONFLICT (content): Merge conflict in f.txt"
        );
        assert_eq!(
            run("Auto-merging f.txt\n", "error: could not apply 53a4d25\n").message(),
            "Auto-merging f.txt\nerror: could not apply 53a4d25"
        );
        assert_eq!(run("  \n", "\n").message(), "");
    }

    #[test]
    fn a_failure_with_an_empty_stderr_reports_stdout_instead() {
        let run = Run {
            success: false,
            code: Some(1),
            stdout: "Automatic merge failed".to_string(),
            stderr: String::new(),
        };
        let Err(GitError::Failed { stderr, .. }) = run.require_success("merge") else {
            panic!("expected a failure");
        };
        assert_eq!(stderr, "Automatic merge failed");
    }
}
