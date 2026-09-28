//! The authenticated API: what a paired phone can ask the desktop to do.
//!
//! Every request names a pull request by repository and branch rather than by a
//! path on the desktop. The phone never learns more of the desktop's filesystem
//! than the worktree paths it is shown, and it cannot name a directory the
//! desktop's config does not already list as a clone.

use chrono::{DateTime, Utc};
use rostrum_core::{PrNumber, RepoId};
use serde::{Deserialize, Serialize};

/// `GET /api/v1/machine`: the desktop, as far as the phone needs to know it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineInfo {
    pub name: String,
    /// `rostrumd`'s version.
    pub version: String,
    pub api_version: u32,
    /// Repositories with a local clone configured on the desktop.
    pub clones: Vec<CloneInfo>,
    /// Whether a stopped rebase or merge is handed to a tmux session rather
    /// than aborted.
    pub handler_configured: bool,
    /// The desktop's saved `autostash` preference, the phone's default.
    pub autostash: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneInfo {
    pub repo: RepoId,
    /// The configured path, tilde-expanded, for display.
    pub path: String,
}

/// A pull request's identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrKey {
    pub repo: RepoId,
    pub number: PrNumber,
}

/// A pull request with what a local job needs to know about it: the branches,
/// and what a conflict handoff's context bundle describes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrRef {
    pub key: PrKey,
    pub title: String,
    pub url: String,
    pub body: String,
    pub head_ref: String,
    pub base_ref: String,
}

/// `POST /api/v1/local/status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStatusRequest {
    pub key: PrKey,
    pub head_ref: String,
}

/// The local half of a pull request, as the desktop sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalStatus {
    /// No clone is configured for this repository on the desktop.
    NotConfigured,
    /// The clone exists but no worktree has the branch checked out.
    NotCheckedOut,
    CheckedOut {
        branch: LocalBranchStatus,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBranchStatus {
    pub worktree: String,
    pub branch: String,
    /// Local commits not on `origin/<branch>`: work not pushed yet.
    pub ahead: u32,
    /// Commits on `origin/<branch>` not local: work not pulled yet.
    pub behind: u32,
    /// Whether the counts come from a fetch made just now.
    pub fetched: bool,
    /// Why a local action cannot run, if anything is in the way.
    pub blocker: Option<String>,
    pub in_progress: Option<InProgressKind>,
    pub handoff: Option<HandoffStatus>,
}

/// A multi-step git operation stopped part-way in the worktree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InProgressKind {
    Rebase,
    Am,
    Merge,
    CherryPick,
    Revert,
    Bisect,
}

impl InProgressKind {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Rebase => "a rebase is in progress",
            Self::Am => "`git am` is in progress",
            Self::Merge => "a merge is in progress",
            Self::CherryPick => "a cherry-pick is in progress",
            Self::Revert => "a revert is in progress",
            Self::Bisect => "a bisect is in progress",
        }
    }

    /// Whether [`crate::routes::LOCAL_ABORT`] can undo it. Only rebases and
    /// merges are started by rostrum, and only they are offered an abort.
    pub fn abortable(self) -> bool {
        matches!(self, Self::Rebase | Self::Merge)
    }
}

/// Whether a handed-off conflict still has someone working on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandoffStatus {
    Running {
        session: String,
    },
    /// Mid-operation, but the tmux session is gone.
    Gone {
        session: String,
    },
}

impl HandoffStatus {
    pub fn session(&self) -> &str {
        match self {
            Self::Running { session } | Self::Gone { session } => session,
        }
    }

    /// What to type on the desktop to join the session. `=` makes tmux match
    /// the name exactly rather than as a prefix.
    pub fn attach_command(&self) -> String {
        attach_command(self.session())
    }
}

pub fn attach_command(session: &str) -> String {
    format!("tmux attach -t ={session}")
}

/// Which local operation to run, and so against which ref.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOpKind {
    /// Fetch `origin/<head>` and rebase local commits onto it.
    PullRebase,
    /// Merge `origin/<head>` into the local branch.
    MergeRemote,
    /// Merge `origin/<base>` into the local branch.
    MergeBase,
    /// Rebase the local branch onto `origin/<base>`.
    RebaseBase,
}

impl LocalOpKind {
    pub const ALL: [Self; 4] = [
        Self::PullRebase,
        Self::MergeRemote,
        Self::MergeBase,
        Self::RebaseBase,
    ];

    /// Button text.
    pub fn label(self) -> &'static str {
        match self {
            Self::PullRebase => "Pull (rebase)",
            Self::MergeRemote => "Merge remote",
            Self::MergeBase => "Merge base",
            Self::RebaseBase => "Rebase onto base",
        }
    }

    /// Sync-all button text.
    pub fn all_label(self) -> &'static str {
        match self {
            Self::PullRebase => "Pull all",
            Self::MergeRemote => "Merge remote into all",
            Self::MergeBase => "Merge base into all",
            Self::RebaseBase => "Rebase all onto base",
        }
    }
}

/// `POST /api/v1/local/job`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRequest {
    pub pr: PrRef,
    pub op: LocalOpKind,
    pub autostash: bool,
}

/// What one local job did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobOutcome {
    NotConfigured,
    NotCheckedOut,
    UpToDate,
    Completed,
    /// git declined to start; the repository is untouched.
    Refused {
        reason: String,
    },
    /// Stopped on a conflict and aborted; the repository is as it was.
    Conflicted {
        reason: String,
    },
    /// Stopped on a conflict and left for the named tmux session.
    HandedOff {
        session: String,
    },
    Failed {
        reason: String,
    },
}

impl JobOutcome {
    /// Chip text, or `None` for the unremarkable outcomes.
    pub fn chip(&self) -> Option<&'static str> {
        match self {
            Self::NotConfigured | Self::NotCheckedOut | Self::UpToDate | Self::Completed => None,
            Self::Refused { .. } => Some("refused"),
            Self::Conflicted { .. } => Some("conflict"),
            Self::HandedOff { .. } => Some("handed off"),
            Self::Failed { .. } => Some("failed"),
        }
    }

    /// The sentence behind the chip.
    pub fn detail(&self) -> String {
        match self {
            Self::NotConfigured => "no clone is configured for this repository".into(),
            Self::NotCheckedOut => "not checked out in any worktree".into(),
            Self::UpToDate => "already up to date".into(),
            Self::Completed => "updated".into(),
            Self::Refused { reason } | Self::Conflicted { reason } | Self::Failed { reason } => {
                reason.clone()
            }
            Self::HandedOff { session } => {
                format!("handed off to tmux — {}", attach_command(session))
            }
        }
    }
}

/// `POST /api/v1/local/abort`: abort what is in progress where `head_ref` is
/// checked out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbortRequest {
    pub key: PrKey,
    pub head_ref: String,
}

/// `POST /api/v1/sync-all`: run `op` on every listed pull request with a
/// worktree, one at a time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncAllRequest {
    pub op: LocalOpKind,
    pub autostash: bool,
    pub prs: Vec<PrRef>,
}

/// A sync-all run, running or finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRun {
    pub id: u64,
    pub op: LocalOpKind,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub entries: Vec<SyncEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncEntry {
    pub key: PrKey,
    pub head_ref: String,
    pub state: SyncEntryState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncEntryState {
    Pending,
    Running,
    Done { outcome: JobOutcome },
}

/// Counts per outcome bucket, for the progress line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

impl SyncRun {
    pub fn is_finished(&self) -> bool {
        self.finished_at.is_some()
    }

    pub fn summary(&self) -> SyncSummary {
        let mut summary = SyncSummary {
            total: u32::try_from(self.entries.len()).unwrap_or(u32::MAX),
            ..SyncSummary::default()
        };
        for entry in &self.entries {
            let SyncEntryState::Done { outcome } = &entry.state else {
                continue;
            };
            summary.done += 1;
            let bucket = match outcome {
                JobOutcome::Completed => &mut summary.updated,
                JobOutcome::UpToDate => &mut summary.up_to_date,
                JobOutcome::HandedOff { .. } => &mut summary.handed_off,
                JobOutcome::Conflicted { .. } => &mut summary.conflicts,
                JobOutcome::Refused { .. } => &mut summary.refused,
                JobOutcome::Failed { .. } => &mut summary.failed,
                JobOutcome::NotCheckedOut | JobOutcome::NotConfigured => &mut summary.skipped,
            };
            *bucket += 1;
        }
        summary
    }
}

impl SyncSummary {
    /// `3/12…` while running; `9 updated, 2 handed off, 1 conflict` after,
    /// with empty buckets left out.
    pub fn describe(&self, finished: bool) -> String {
        if !finished {
            return format!("{}/{}\u{2026}", self.done, self.total);
        }
        let parts: Vec<String> = [
            (self.updated, "updated"),
            (self.up_to_date, "up to date"),
            (self.handed_off, "handed off"),
            (self.conflicts, "conflicts"),
            (self.refused, "refused"),
            (self.failed, "failed"),
            (self.skipped, "not checked out"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect();
        if parts.is_empty() {
            "nothing to do".into()
        } else {
            parts.join(", ")
        }
    }
}

/// `GET /api/v1/handoffs`: a tmux session rostrum handed a conflict to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffSession {
    pub session: String,
    /// The pull request, when the desktop started this session itself.
    pub key: Option<PrKey>,
    pub head_ref: Option<String>,
    pub worktree: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

impl HandoffSession {
    pub fn attach_command(&self) -> String {
        attach_command(&self.session)
    }
}

/// The body of every non-2xx response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    /// No token, or a token the desktop does not know (revoked).
    Unauthorized,
    /// The caller may not do this from where it is.
    Forbidden,
    BadRequest,
    NotFound,
    /// The pairing code is wrong, already used, or never existed.
    PairingCodeInvalid,
    PairingCodeExpired,
    /// Too many failed attempts; try again later.
    RateLimited,
    /// Another job or a sync-all is already running on that worktree.
    Busy,
    Internal,
}

impl ApiErrorCode {
    pub fn http_status(self) -> u16 {
        match self {
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::PairingCodeInvalid => 403,
            Self::PairingCodeExpired => 410,
            Self::RateLimited => 429,
            Self::Busy => 409,
            Self::Internal => 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u32) -> PrKey {
        PrKey {
            repo: RepoId::new("RhizoNymph", "rostrum"),
            number: PrNumber(n),
        }
    }

    fn entry(n: u32, state: SyncEntryState) -> SyncEntry {
        SyncEntry {
            key: key(n),
            head_ref: format!("branch-{n}"),
            state,
        }
    }

    fn done(outcome: JobOutcome) -> SyncEntryState {
        SyncEntryState::Done { outcome }
    }

    #[test]
    fn outcomes_serialise_with_a_kind_tag() {
        let json = serde_json::to_value(JobOutcome::HandedOff {
            session: "rostrum-a-b-1".into(),
        })
        .expect("serialises");
        assert_eq!(
            json,
            serde_json::json!({"kind": "handed_off", "session": "rostrum-a-b-1"})
        );
        let json = serde_json::to_value(JobOutcome::UpToDate).expect("serialises");
        assert_eq!(json, serde_json::json!({"kind": "up_to_date"}));
    }

    #[test]
    fn local_status_round_trips() {
        let status = LocalStatus::CheckedOut {
            branch: LocalBranchStatus {
                worktree: "/home/u/Code/rostrum/feat-x".into(),
                branch: "feat/x".into(),
                ahead: 2,
                behind: 0,
                fetched: true,
                blocker: None,
                in_progress: Some(InProgressKind::Rebase),
                handoff: Some(HandoffStatus::Running {
                    session: "rostrum-RhizoNymph-rostrum-10".into(),
                }),
            },
        };
        let json = serde_json::to_string(&status).expect("serialises");
        assert_eq!(
            serde_json::from_str::<LocalStatus>(&json).expect("parses"),
            status
        );
    }

    #[test]
    fn only_the_remarkable_outcomes_get_a_chip() {
        assert_eq!(JobOutcome::Completed.chip(), None);
        assert_eq!(JobOutcome::UpToDate.chip(), None);
        assert_eq!(JobOutcome::NotCheckedOut.chip(), None);
        assert_eq!(
            JobOutcome::Refused { reason: "x".into() }.chip(),
            Some("refused")
        );
        assert_eq!(
            JobOutcome::HandedOff {
                session: "s".into()
            }
            .chip(),
            Some("handed off")
        );
    }

    #[test]
    fn a_handoff_tells_you_how_to_attach_exactly() {
        let status = HandoffStatus::Gone {
            session: "rostrum-a-b-1".into(),
        };
        assert_eq!(status.attach_command(), "tmux attach -t =rostrum-a-b-1");
        assert!(
            JobOutcome::HandedOff {
                session: "rostrum-a-b-1".into()
            }
            .detail()
            .contains("tmux attach -t =rostrum-a-b-1")
        );
    }

    #[test]
    fn a_running_sync_counts_only_finished_entries() {
        let run = SyncRun {
            id: 1,
            op: LocalOpKind::PullRebase,
            started_at: Utc::now(),
            finished_at: None,
            entries: vec![
                entry(1, done(JobOutcome::Completed)),
                entry(2, SyncEntryState::Running),
                entry(3, SyncEntryState::Pending),
            ],
        };
        let summary = run.summary();
        assert_eq!((summary.total, summary.done, summary.updated), (3, 1, 1));
        assert_eq!(summary.describe(run.is_finished()), "1/3\u{2026}");
    }

    #[test]
    fn a_finished_sync_names_only_the_non_empty_buckets() {
        let run = SyncRun {
            id: 2,
            op: LocalOpKind::RebaseBase,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            entries: vec![
                entry(1, done(JobOutcome::Completed)),
                entry(2, done(JobOutcome::Completed)),
                entry(
                    3,
                    done(JobOutcome::HandedOff {
                        session: "s".into(),
                    }),
                ),
                entry(
                    4,
                    done(JobOutcome::Refused {
                        reason: "dirty".into(),
                    }),
                ),
                entry(5, done(JobOutcome::NotCheckedOut)),
            ],
        };
        assert_eq!(
            run.summary().describe(true),
            "2 updated, 1 handed off, 1 refused, 1 not checked out"
        );
    }

    #[test]
    fn an_empty_finished_sync_says_so() {
        assert_eq!(SyncSummary::default().describe(true), "nothing to do");
    }

    #[test]
    fn only_rebases_and_merges_are_abortable() {
        assert!(InProgressKind::Rebase.abortable());
        assert!(InProgressKind::Merge.abortable());
        assert!(!InProgressKind::Bisect.abortable());
        assert!(!InProgressKind::Am.abortable());
    }

    #[test]
    fn every_error_code_maps_to_a_client_or_server_status() {
        for code in [
            ApiErrorCode::Unauthorized,
            ApiErrorCode::Forbidden,
            ApiErrorCode::BadRequest,
            ApiErrorCode::NotFound,
            ApiErrorCode::PairingCodeInvalid,
            ApiErrorCode::PairingCodeExpired,
            ApiErrorCode::RateLimited,
            ApiErrorCode::Busy,
        ] {
            assert!((400..500).contains(&code.http_status()), "{code:?}");
        }
        assert_eq!(ApiErrorCode::Internal.http_status(), 500);
    }

    #[test]
    fn api_errors_serialise_with_a_snake_case_code() {
        let json = serde_json::to_value(ApiError::new(ApiErrorCode::PairingCodeExpired, "late"))
            .expect("serialises");
        assert_eq!(
            json,
            serde_json::json!({"code": "pairing_code_expired", "message": "late"})
        );
    }

    #[test]
    fn every_op_has_distinct_labels() {
        let labels: std::collections::HashSet<_> =
            LocalOpKind::ALL.iter().map(|op| op.label()).collect();
        assert_eq!(labels.len(), 4);
    }
}
