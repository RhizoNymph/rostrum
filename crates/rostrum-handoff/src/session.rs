//! Starting the handler in a tmux session the user can attach to.
//!
//! # Why the environment is inherited, not rebuilt
//!
//! This is the mirror image of `rostrum-git/src/command.rs`, and deliberately
//! so. rostrum-git clears the environment and puts back an allowlist because
//! it *parses git's output* and `GIT_DIR`, `GIT_WORK_TREE`, or an askpass
//! helper change what git says or does; a stripped environment is what makes
//! its answers trustworthy. The tmux client parses nothing. It relays an argv
//! to a server and exits. What eventually runs is the user's own interactive
//! tool — `claude`, an editor, a shell — and that tool needs exactly what
//! rostrum-git drops: `DISPLAY` for browser-based auth, the real `TERM` so
//! the pane renders, `ANTHROPIC_*` and friends for the harness, the full
//! `PATH` the user's shell would have had.
//!
//! Two facts about tmux settle it. First, if this call is what starts the
//! server, the server's *global environment* is a copy of the client's, and a
//! stripped one would poison every later session the user opens from any
//! terminal. Second, when a server already exists only the variables named in
//! `update-environment` refresh from the client, so the interactive shell's
//! rc files — not anything we pass — are what make the pane's environment
//! right. Either way, stripping would only ever hurt.
//!
//! The single variable removed is `TMUX`: a rostrum launched from inside a
//! tmux pane would otherwise trip the nesting check ("sessions should be
//! nested with care") and refuse to start a detached session.
//!
//! # Why the command is typed into a shell
//!
//! `tmux new-session <cmd>` would be shorter. It is worse in three ways, all
//! covered on [`spawn_argv`].

use std::{path::Path, process::Stdio, time::Duration};

use rostrum_core::{PrNumber, RepoId};
use tokio::process::Command;

use crate::error::HandoffError;

/// What [`spawn`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spawned {
    /// A new session was started and the command typed into it.
    Started,
    /// A session by that name already existed; nothing was typed into it.
    /// The user is mid-resolution, or a harness is still running.
    AlreadyRunning,
}

/// The tmux session name for a pull request: `rostrum-<owner>-<name>-<n>`,
/// passed through [`sanitise`].
///
/// One identity is used for the session and the bundle file, so "is there a
/// handler for this PR" and "where is its bundle" are the same question.
pub fn session_name(repo: &RepoId, number: PrNumber) -> String {
    sanitise(&format!(
        "rostrum-{}-{}-{}",
        repo.owner(),
        repo.name(),
        number.0
    ))
}

/// Replace every character outside `[A-Za-z0-9_-]` with `_`.
///
/// `.` and `:` are the ones that matter. tmux (3.0 and later) silently
/// rewrites both to `_` in a session name, because `:` separates a session
/// from a window in a target and `.` separates a window from a pane. If we
/// asked for `x.y` and tmux stored `x_y`, `has-session -t =x.y` would say "no
/// session" forever and every click would spawn a duplicate. Doing the
/// rewrite ourselves means the name we ask about is the name tmux kept.
/// Everything else non-alphanumeric is folded the same way so a unicode owner
/// or a name with a space needs no quoting anywhere.
pub fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The single tmux invocation that starts a session and types `command` into
/// it, as an argv (no shell involved on our side).
///
/// ```text
/// new-session -d -s <name> -c <worktree> ;
/// send-keys -t =<name>: -l -- <command> ;
/// send-keys -t =<name>: Enter
/// ```
///
/// Each `;` is its own argv element: it is tmux's command separator, and
/// three commands in one client call are atomic with respect to other
/// clients.
///
/// **The target is `=<name>:`**, both characters load-bearing. `send-keys`
/// takes a *pane* target, and tmux resolves a bare `=<name>` as a pane
/// name, which fails with "can't find pane" even though the session exists.
/// The trailing `:` makes it "the current window of session `<name>`", and
/// the leading `=` keeps the session match exact — without it `x-1` is a
/// prefix match for `x-12`, and the command would be typed into the wrong
/// pull request's session. Verified against tmux 3.6.
///
/// **Why not `new-session <command>`?** Three reasons. (1) A harness that
/// fails to start — `claude: command not found` — would have tmux destroy
/// the session the instant the command exits, leaving nothing to look at.
/// Typed into a shell, the failure stays on screen. (2) The interactive
/// shell reads its rc files, so the command runs with the user's real `PATH`
/// and environment rather than whatever a GUI launcher inherited. (3) The
/// template is interpreted by the user's own shell; there is no quoting layer
/// of ours between the configured text and what runs.
///
/// **`-l`** makes `send-keys` type the argument literally instead of looking
/// it up as a key name, so a command that happens to be the word `Enter` is
/// typed and not pressed. **`--`** ends option parsing so a command starting
/// with `-` is not read as a flag.
///
/// **Trailing `;`.** tmux treats a trailing `;` on *any* argument as a command
/// separator, even after `--`. A command ending in `;` (`echo hi;`) would be
/// split into `send-keys … 'echo hi'` followed by an empty command. tmux's
/// escape for this is `\;`, so the last character is rewritten to that; the
/// shell then receives the literal `;` it was meant to.
pub fn spawn_argv(name: &str, worktree: &Path, command: &str) -> Vec<String> {
    let target = format!("={name}:");
    let command = match command.strip_suffix(';') {
        Some(head) => format!("{head}\\;"),
        None => command.to_string(),
    };
    vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        name.to_string(),
        "-c".to_string(),
        worktree.display().to_string(),
        ";".to_string(),
        "send-keys".to_string(),
        "-t".to_string(),
        target.clone(),
        "-l".to_string(),
        "--".to_string(),
        command,
        ";".to_string(),
        "send-keys".to_string(),
        "-t".to_string(),
        target,
        "Enter".to_string(),
    ]
}

/// What one tmux client invocation produced.
struct Run {
    success: bool,
    code: Option<i32>,
    stderr: String,
}

/// Run one `tmux` client invocation to completion, or kill it.
///
/// See the module doc for why the environment is inherited. `stdin` is null
/// because the client has nothing to read and a client that finds a terminal
/// on stdin will try to attach to it.
async fn run(args: &[String], timeout: Duration) -> Result<Run, HandoffError> {
    let mut command = Command::new("tmux");
    command.args(args);
    command.env_remove("TMUX");
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    // The timeout below drops the child; this is what turns that into a kill.
    command.kill_on_drop(true);

    let child = command.spawn().map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            HandoffError::TmuxMissing { source }
        } else {
            HandoffError::TmuxFailed {
                args: args.join(" "),
                code: None,
                stderr: source.to_string(),
            }
        }
    })?;

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(source)) => {
            return Err(HandoffError::TmuxFailed {
                args: args.join(" "),
                code: None,
                stderr: source.to_string(),
            });
        }
        Err(_) => return Err(HandoffError::TmuxTimeout { after: timeout }),
    };

    let run = Run {
        success: output.status.success(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    };
    tracing::debug!(args = %args.join(" "), code = ?run.code, "ran tmux");
    Ok(run)
}

/// Whether a session named exactly `name` exists.
///
/// `tmux has-session -t =<name>`: the `=` prefix forces an exact match, so
/// `x-1` never prefix-hits `x-12`. Exit 0 is "yes"; exit 1 is "no", which is
/// also what a missing server answers, and deliberately not special-cased —
/// no server and no session are the same fact for a caller about to start
/// one.
pub async fn session_exists(name: &str, timeout: Duration) -> Result<bool, HandoffError> {
    let args = ["has-session", "-t", &format!("={name}")].map(String::from);
    let run = run(&args, timeout).await?;
    match run.code {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(HandoffError::TmuxFailed {
            args: args.join(" "),
            code,
            stderr: run.stderr,
        }),
    }
}

/// Start a session named `name` in `worktree` and type `command` into it.
///
/// Checks for an existing session first, and again after a failed start: two
/// clicks in quick succession can both see "no session" and both try, and
/// the loser of that race has nothing to report except that the session is
/// there.
pub async fn spawn(
    name: &str,
    worktree: &Path,
    command: &str,
    timeout: Duration,
) -> Result<Spawned, HandoffError> {
    if session_exists(name, timeout).await? {
        tracing::info!(session = name, "handoff session already running");
        return Ok(Spawned::AlreadyRunning);
    }

    let args = spawn_argv(name, worktree, command);
    let run = run(&args, timeout).await?;
    if run.success {
        tracing::info!(session = name, worktree = %worktree.display(), "started handoff session");
        return Ok(Spawned::Started);
    }

    if session_exists(name, timeout).await? {
        tracing::info!(
            session = name,
            code = ?run.code,
            stderr = %run.stderr,
            "lost the race to start the session; it is running"
        );
        return Ok(Spawned::AlreadyRunning);
    }
    Err(HandoffError::TmuxFailed {
        args: args.join(" "),
        code: run.code,
        stderr: run.stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_name_is_prefixed_and_carries_owner_name_and_number() {
        let repo = RepoId::new("zed-industries", "zed");
        assert_eq!(
            session_name(&repo, PrNumber(1234)),
            "rostrum-zed-industries-zed-1234"
        );
    }

    /// tmux would rewrite the `.` itself; doing it first keeps the name we
    /// query equal to the name tmux stored.
    #[test]
    fn dots_in_a_repository_name_become_underscores() {
        let repo = RepoId::new("nymph", "rostrum.rs");
        assert_eq!(
            session_name(&repo, PrNumber(7)),
            "rostrum-nymph-rostrum_rs-7"
        );
    }

    #[test]
    fn a_unicode_owner_is_folded_to_underscores() {
        let repo = RepoId::new("héllo:wörld", "x");
        assert_eq!(session_name(&repo, PrNumber(1)), "rostrum-h_llo_w_rld-x-1");
    }

    #[test]
    fn the_argv_starts_a_detached_session_and_types_the_command_into_it() {
        let argv = spawn_argv("s", Path::new("/w"), "claude 'fix {c}'");
        assert_eq!(
            argv,
            vec![
                "new-session",
                "-d",
                "-s",
                "s",
                "-c",
                "/w",
                ";",
                "send-keys",
                "-t",
                "=s:",
                "-l",
                "--",
                "claude 'fix {c}'",
                ";",
                "send-keys",
                "-t",
                "=s:",
                "Enter",
            ]
        );
    }

    /// A trailing `;` is a separator to tmux even after `--`.
    #[test]
    fn a_trailing_semicolon_is_escaped_for_tmux() {
        let argv = spawn_argv("s", Path::new("/w"), "echo hi;");
        assert_eq!(argv[12], "echo hi\\;");
        // Only the last one: an interior `;` is not a separator.
        let argv = spawn_argv("s", Path::new("/w"), "a; b");
        assert_eq!(argv[12], "a; b");
    }

    #[test]
    fn a_command_that_is_only_a_semicolon_becomes_an_escaped_one() {
        let argv = spawn_argv("s", Path::new("/w"), ";");
        assert_eq!(argv[12], "\\;");
    }

    /// `--` is what keeps this from being read as an option to `send-keys`.
    #[test]
    fn a_command_starting_with_a_dash_follows_the_option_terminator() {
        let argv = spawn_argv("s", Path::new("/w"), "-n foo");
        assert_eq!(argv[11], "--");
        assert_eq!(argv[12], "-n foo");
    }
}
