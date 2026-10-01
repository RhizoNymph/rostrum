//! Pairing with the desktop, and the desktop's view of local clones.

use std::time::SystemTime;

use crate::types::Chip;

/// What a pairing link says, shown before pairing so the user can check it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairingPreview {
    /// The desktop's machine name.
    pub machine: String,
    /// Every address the desktop listens on, in the order they are tried.
    pub hosts: Vec<String>,
    pub port: u16,
    /// `4F2A · 91C0 · 7E3B`: compare with the desktop's pairing page.
    pub fingerprint_short: String,
    /// The pairing code as the desktop shows it, `XXXX-XXXX`.
    pub code: String,
}

/// What an unpaired desktop said when asked by address, for manual pairing.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DesktopProbe {
    pub machine: String,
    pub api_version: u32,
    /// Whether this build speaks the desktop's protocol version.
    pub compatible: bool,
    pub host: String,
    pub port: u16,
    /// The certificate fingerprint seen, to pass to `pair_manual` once the
    /// user has compared `fingerprint_short` with the desktop's page.
    pub fingerprint: String,
    pub fingerprint_short: String,
}

/// A completed pairing. Persist `endpoint` anywhere and `device_token` (and
/// `github.token`) in the Keystore; hand them back with `set_remote` and
/// `set_github_token` on the next launch.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairingResult {
    pub machine: MachineInfo,
    /// The desktop's addresses, port and certificate fingerprint, serialised.
    /// Not a secret.
    pub endpoint: String,
    pub device_id: String,
    /// The bearer credential for this device. A secret.
    pub device_token: String,
    /// The desktop's GitHub token, when it chose to hand one over. A secret.
    pub github: Option<DesktopGitHubToken>,
}

/// Redacted: a stray `{:?}` must not put the device token in a log.
impl std::fmt::Debug for PairingResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingResult")
            .field("machine", &self.machine)
            .field("endpoint", &self.endpoint)
            .field("device_id", &self.device_id)
            .field("device_token", &"redacted")
            .field("github", &self.github)
            .finish()
    }
}

/// A GitHub token handed over by the desktop.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct DesktopGitHubToken {
    pub token: String,
    /// Where the desktop got it, e.g. `gh auth token`.
    pub source: String,
    pub host: String,
}

impl std::fmt::Debug for DesktopGitHubToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopGitHubToken")
            .field("token", &"redacted")
            .field("source", &self.source)
            .field("host", &self.host)
            .finish()
    }
}

/// Whether a desktop is set for this session.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteStatus {
    NotPaired,
    Paired {
        hosts: Vec<String>,
        port: u16,
        fingerprint_short: String,
        /// The address that answered most recently.
        current_host: String,
    },
}

/// The paired desktop.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MachineInfo {
    pub name: String,
    pub version: String,
    pub api_version: u32,
    /// Repositories with a local clone on the desktop.
    pub clones: Vec<CloneInfo>,
    /// Whether a stopped rebase or merge is handed to a tmux session there.
    pub handler_configured: bool,
    /// The desktop's own stash preference.
    pub autostash: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CloneInfo {
    /// `owner/name`.
    pub repo: String,
    pub path: String,
}

/// The desktop's clone's view of one pull request's branch.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LocalStatus {
    /// No clone of this repository on the desktop.
    NotConfigured,
    /// A clone exists, but no worktree has the branch checked out.
    NotCheckedOut,
    CheckedOut {
        branch: LocalBranch,
    },
}

/// A checked-out branch on the desktop.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LocalBranch {
    pub worktree: String,
    pub branch: String,
    /// Local commits not on `origin/<branch>`: push them from the desktop.
    pub ahead: u32,
    /// Remote commits not in the local branch.
    pub behind: u32,
    /// Whether the fetch before counting succeeded; if not, the counts
    /// describe what was on disk.
    pub fetched: bool,
    /// Why the local operations would refuse to start (a dirty worktree
    /// without stash, …).
    pub blocker: Option<String>,
    /// A rebase or merge stopped part-way.
    pub in_progress: Option<InProgress>,
    /// The tmux session a stopped operation was handed to.
    pub handoff: Option<HandoffState>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InProgress {
    pub kind: InProgressKind,
    /// "a rebase is in progress".
    pub description: String,
    /// Whether `abort_local` can abort it (rebase and merge only).
    pub abortable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InProgressKind {
    Rebase,
    Am,
    Merge,
    CherryPick,
    Revert,
    Bisect,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HandoffState {
    pub session: String,
    /// Whether the tmux session still exists.
    pub running: bool,
    /// `tmux attach -t =<session>`.
    pub attach_command: String,
}

/// One local git operation on the desktop. Nothing is ever pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LocalOp {
    /// Rebase local commits onto `origin/<branch>`.
    PullRebase,
    /// Merge `origin/<branch>` into the local branch.
    MergeRemote,
    /// Merge `origin/<base>` into the branch.
    MergeBase,
    /// Rebase the branch onto `origin/<base>`.
    RebaseBase,
}

/// The operations "sync all" can run across every checked-out branch.
/// Merging the remote into every worktree is a per-branch decision, so it is
/// not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncAllOp {
    Pull,
    MergeBase,
    RebaseBase,
}

/// How a local job ended.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum JobOutcome {
    NotConfigured,
    NotCheckedOut,
    UpToDate,
    Completed,
    /// Git would not start (dirty worktree, operation in progress, …).
    Refused {
        reason: String,
    },
    /// Stopped on conflicts and aborted; the worktree is as it was.
    Conflicted {
        reason: String,
    },
    /// Stopped on conflicts and handed to a tmux session on the desktop.
    HandedOff {
        session: String,
        attach_command: String,
    },
    Failed {
        reason: String,
    },
}

/// A job's outcome with its render-ready description.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JobResult {
    pub outcome: JobOutcome,
    /// One line for a snackbar.
    pub detail: String,
    /// `refused`, `conflict`, `handed off`, `failed`; `None` for success.
    pub chip: Option<Chip>,
}

/// A "sync all" run on the desktop, running or finished.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncRun {
    pub id: u64,
    pub op: LocalOp,
    pub started_at: SystemTime,
    pub finished_at: Option<SystemTime>,
    pub entries: Vec<SyncEntry>,
    pub summary: SyncSummary,
    /// `3/12…` while running; `9 updated, 2 handed off` when done.
    pub progress_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncEntry {
    pub repo: String,
    pub number: u32,
    pub head_ref: String,
    pub state: SyncEntryState,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SyncEntryState {
    Pending,
    Running,
    Done { result: JobResult },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SyncSummary {
    pub total: u32,
    pub done: u32,
    pub updated: u32,
    pub up_to_date: u32,
    pub handed_off: u32,
    pub conflicts: u32,
    pub refused: u32,
    pub failed: u32,
    pub skipped: u32,
}

/// A tmux session on the desktop holding a stopped rebase or merge.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HandoffSession {
    pub session: String,
    pub repo: Option<String>,
    pub number: Option<u32>,
    pub head_ref: Option<String>,
    pub worktree: Option<String>,
    pub started_at: Option<SystemTime>,
    pub attach_command: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let result = PairingResult {
            machine: MachineInfo {
                name: "desk".into(),
                version: "1".into(),
                api_version: 1,
                clones: vec![],
                handler_configured: false,
                autostash: false,
            },
            endpoint: "{}".into(),
            device_id: "device".into(),
            device_token: "device-secret".into(),
            github: Some(DesktopGitHubToken {
                token: "ghp_secret".into(),
                source: "gh".into(),
                host: "github.com".into(),
            }),
        };
        let printed = format!("{result:?}");
        assert!(!printed.contains("device-secret"), "{printed}");
        assert!(!printed.contains("ghp_secret"), "{printed}");
        assert!(printed.contains("device"));
    }
}
