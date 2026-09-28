//! The tmux sessions conflicts were handed to.
//!
//! `tmux list-sessions` is the truth about which sessions exist; the daemon's
//! [`HandoffRecord`]s add which pull request and worktree each was for. Only
//! sessions named `rostrum-…` are listed — the user's other sessions are none
//! of the phone's business. No tmux server (or no tmux at all) means no
//! sessions, not an error.

use std::{process::Stdio, time::Duration};

use chrono::{DateTime, Utc};
use rostrum_remote::HandoffSession;

use crate::{boxed::BoxFuture, jobs::HandoffRecord};

/// Every session rostrum starts is named with this prefix
/// (`rostrum_handoff::session_name`).
pub const PREFIX: &str = "rostrum-";
/// The tmux client relays to a server and exits; anything slower is wedged.
pub const TIMEOUT: Duration = Duration::from_secs(5);
const FORMAT: &str = "#{session_name} #{session_created}";

/// A session as tmux lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmuxSession {
    pub name: String,
    pub created: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("could not run `tmux`: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("`tmux list-sessions` timed out")]
    Timeout,
    #[error("`tmux list-sessions` failed{}: {stderr}", code.map(|c| format!(" (exit {c})")).unwrap_or_default())]
    Failed { code: Option<i32>, stderr: String },
}

/// Lists tmux sessions. The daemon uses [`TmuxCli`]; tests supply a list.
pub trait SessionLister: Send + Sync {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TmuxSession>, TmuxError>>;
}

/// The `tmux` command line.
pub struct TmuxCli;

impl SessionLister for TmuxCli {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TmuxSession>, TmuxError>> {
        Box::pin(list_sessions())
    }
}

async fn list_sessions() -> Result<Vec<TmuxSession>, TmuxError> {
    let spawned = tokio::process::Command::new("tmux")
        .args(["list-sessions", "-F", FORMAT])
        // Launched from inside tmux, the client would otherwise talk to that
        // server's socket by way of $TMUX; the default socket is the one
        // rostrum-handoff spawns sessions on.
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let child = match spawned {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("tmux is not installed; no handoff sessions");
            return Ok(Vec::new());
        }
        Err(err) => return Err(TmuxError::Spawn(err)),
    };
    let output = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| TmuxError::Timeout)?
        .map_err(TmuxError::Spawn)?;
    if output.status.success() {
        return Ok(parse_sessions(&String::from_utf8_lossy(&output.stdout)));
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if is_no_server(&stderr) {
        return Ok(Vec::new());
    }
    Err(TmuxError::Failed {
        code: output.status.code(),
        stderr,
    })
}

/// Parse `#{session_name} #{session_created}` lines. A session name may
/// contain spaces; the creation time is the last field.
pub fn parse_sessions(stdout: &str) -> Vec<TmuxSession> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match line.rsplit_once(' ') {
            Some((name, created)) => TmuxSession {
                name: name.to_string(),
                created: created
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .and_then(|secs| DateTime::from_timestamp(secs, 0)),
            },
            None => TmuxSession {
                name: line.to_string(),
                created: None,
            },
        })
        .collect()
}

/// What tmux says when there is no server to ask: the socket is missing
/// ("error connecting to …") or stale ("no server running on …").
pub fn is_no_server(stderr: &str) -> bool {
    stderr.contains("no server running") || stderr.contains("error connecting to")
}

/// The `rostrum-` sessions that exist, joined with what the daemon recorded
/// about them. Newest first.
pub fn handoff_sessions(
    sessions: Vec<TmuxSession>,
    records: &[HandoffRecord],
) -> Vec<HandoffSession> {
    let mut out: Vec<HandoffSession> = sessions
        .into_iter()
        .filter(|session| session.name.starts_with(PREFIX))
        .map(|session| {
            let record = records.iter().find(|record| record.session == session.name);
            HandoffSession {
                key: record.map(|record| record.key.clone()),
                head_ref: record.map(|record| record.head_ref.clone()),
                worktree: record.and_then(|record| record.worktree.clone()),
                started_at: session
                    .created
                    .or_else(|| record.map(|record| record.started_at)),
                session: session.name,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| a.session.cmp(&b.session))
    });
    out
}

#[cfg(test)]
mod tests {
    use rostrum_core::{PrNumber, RepoId};
    use rostrum_remote::PrKey;

    use super::*;

    #[test]
    fn list_sessions_output_parses_names_with_spaces_and_times() {
        let sessions = parse_sessions(
            "rostrum-o-r-1 1759060000\nmy work session 1759000000\nodd\n\nbroken notanumber\n",
        );
        assert_eq!(sessions.len(), 4);
        assert_eq!(sessions[0].name, "rostrum-o-r-1");
        assert_eq!(
            sessions[0].created,
            DateTime::from_timestamp(1_759_060_000, 0)
        );
        assert_eq!(sessions[1].name, "my work session");
        assert_eq!(sessions[2].name, "odd");
        assert_eq!(sessions[2].created, None);
        assert_eq!(sessions[3].created, None);
    }

    #[test]
    fn a_missing_server_is_recognised() {
        assert!(is_no_server("no server running on /tmp/tmux-1000/default"));
        assert!(is_no_server(
            "error connecting to /tmp/tmux-1000/default (No such file or directory)"
        ));
        assert!(!is_no_server("unknown option -- x"));
    }

    fn record(session: &str) -> HandoffRecord {
        HandoffRecord {
            session: session.into(),
            key: PrKey {
                repo: RepoId::new("o", "r"),
                number: PrNumber(1),
            },
            head_ref: "feat".into(),
            worktree: Some("/src/r-feat".into()),
            started_at: DateTime::from_timestamp(1_700_000_000, 0).expect("time"),
        }
    }

    #[test]
    fn only_rostrum_sessions_are_listed_joined_with_records_newest_first() {
        let sessions = vec![
            TmuxSession {
                name: "work".into(),
                created: DateTime::from_timestamp(1_759_000_300, 0),
            },
            TmuxSession {
                name: "rostrum-o-r-1".into(),
                created: DateTime::from_timestamp(1_759_000_100, 0),
            },
            TmuxSession {
                name: "rostrum-x-y-2".into(),
                created: DateTime::from_timestamp(1_759_000_200, 0),
            },
        ];
        let listed = handoff_sessions(
            sessions,
            &[record("rostrum-o-r-1"), record("rostrum-gone-3")],
        );
        let names: Vec<&str> = listed.iter().map(|s| s.session.as_str()).collect();
        assert_eq!(names, vec!["rostrum-x-y-2", "rostrum-o-r-1"]);

        let known = &listed[1];
        assert_eq!(known.key.as_ref().map(|k| k.number), Some(PrNumber(1)));
        assert_eq!(known.head_ref.as_deref(), Some("feat"));
        assert_eq!(known.worktree.as_deref(), Some("/src/r-feat"));
        assert_eq!(known.started_at, DateTime::from_timestamp(1_759_000_100, 0));

        let unknown = &listed[0];
        assert_eq!(unknown.key, None);
        assert_eq!(unknown.worktree, None);
        assert_eq!(unknown.attach_command(), "tmux attach -t =rostrum-x-y-2");
    }

    #[test]
    fn no_sessions_is_an_empty_list() {
        assert!(handoff_sessions(Vec::new(), &[record("rostrum-o-r-1")]).is_empty());
    }
}
