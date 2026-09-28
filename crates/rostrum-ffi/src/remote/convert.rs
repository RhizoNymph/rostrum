//! The protocol's types, as Kotlin sees them.
//!
//! `rostrum-remote` owns the wire types so the desktop and the phone cannot
//! disagree about a field; this is only the last step, into UniFFI records,
//! plus the render-ready extras (chip, detail line, progress text).

use rostrum_remote::{
    api::{self, InProgressKind as WireInProgress, LocalOpKind},
    pairing::PairingOffer,
};

use crate::{
    remote::{
        CloneInfo, HandoffSession, HandoffState, InProgress, InProgressKind, JobOutcome, JobResult,
        LocalBranch, LocalOp, LocalStatus, MachineInfo, PairingPreview, SyncAllOp, SyncEntry,
        SyncEntryState, SyncRun, SyncSummary,
    },
    types::{Chip, ColorRole},
};

pub(crate) fn preview(offer: &PairingOffer) -> PairingPreview {
    PairingPreview {
        machine: offer.machine.clone(),
        hosts: offer
            .endpoint
            .hosts()
            .iter()
            .map(ToString::to_string)
            .collect(),
        port: offer.endpoint.port(),
        fingerprint_short: offer.endpoint.fingerprint().short(),
        code: offer.code.to_string(),
    }
}

pub(crate) fn machine(info: api::MachineInfo) -> MachineInfo {
    MachineInfo {
        name: info.name,
        version: info.version,
        api_version: info.api_version,
        clones: info
            .clones
            .into_iter()
            .map(|clone| CloneInfo {
                repo: clone.repo.to_string(),
                path: clone.path,
            })
            .collect(),
        handler_configured: info.handler_configured,
        autostash: info.autostash,
    }
}

pub(crate) fn local_status(status: api::LocalStatus) -> LocalStatus {
    match status {
        api::LocalStatus::NotConfigured => LocalStatus::NotConfigured,
        api::LocalStatus::NotCheckedOut => LocalStatus::NotCheckedOut,
        api::LocalStatus::CheckedOut { branch } => LocalStatus::CheckedOut {
            branch: LocalBranch {
                worktree: branch.worktree,
                branch: branch.branch,
                ahead: branch.ahead,
                behind: branch.behind,
                fetched: branch.fetched,
                blocker: branch.blocker,
                in_progress: branch.in_progress.map(|kind| InProgress {
                    kind: in_progress_kind(kind),
                    description: kind.describe().to_string(),
                    abortable: kind.abortable(),
                }),
                handoff: branch.handoff.map(|handoff| HandoffState {
                    attach_command: handoff.attach_command(),
                    running: matches!(handoff, api::HandoffStatus::Running { .. }),
                    session: handoff.session().to_string(),
                }),
            },
        },
    }
}

fn in_progress_kind(kind: WireInProgress) -> InProgressKind {
    match kind {
        WireInProgress::Rebase => InProgressKind::Rebase,
        WireInProgress::Am => InProgressKind::Am,
        WireInProgress::Merge => InProgressKind::Merge,
        WireInProgress::CherryPick => InProgressKind::CherryPick,
        WireInProgress::Revert => InProgressKind::Revert,
        WireInProgress::Bisect => InProgressKind::Bisect,
    }
}

/// A job's outcome with its chip — red for what stopped, accent for what was
/// handed on, amber for what git would not start: the desktop's colours.
pub(crate) fn job_result(outcome: api::JobOutcome) -> JobResult {
    let detail = outcome.detail();
    let role = match &outcome {
        api::JobOutcome::Conflicted { .. } | api::JobOutcome::Failed { .. } => ColorRole::Danger,
        api::JobOutcome::HandedOff { .. } => ColorRole::Accent,
        _ => ColorRole::Warning,
    };
    let chip = outcome.chip().map(|text| Chip {
        text: text.to_string(),
        role,
        tooltip: Some(detail.clone()),
    });
    let outcome = match outcome {
        api::JobOutcome::NotConfigured => JobOutcome::NotConfigured,
        api::JobOutcome::NotCheckedOut => JobOutcome::NotCheckedOut,
        api::JobOutcome::UpToDate => JobOutcome::UpToDate,
        api::JobOutcome::Completed => JobOutcome::Completed,
        api::JobOutcome::Refused { reason } => JobOutcome::Refused { reason },
        api::JobOutcome::Conflicted { reason } => JobOutcome::Conflicted { reason },
        api::JobOutcome::HandedOff { session } => JobOutcome::HandedOff {
            attach_command: api::attach_command(&session),
            session,
        },
        api::JobOutcome::Failed { reason } => JobOutcome::Failed { reason },
    };
    JobResult {
        outcome,
        detail,
        chip,
    }
}

pub(crate) fn op_kind(op: LocalOp) -> LocalOpKind {
    match op {
        LocalOp::PullRebase => LocalOpKind::PullRebase,
        LocalOp::MergeRemote => LocalOpKind::MergeRemote,
        LocalOp::MergeBase => LocalOpKind::MergeBase,
        LocalOp::RebaseBase => LocalOpKind::RebaseBase,
    }
}

pub(crate) fn sync_op_kind(op: SyncAllOp) -> LocalOpKind {
    match op {
        SyncAllOp::Pull => LocalOpKind::PullRebase,
        SyncAllOp::MergeBase => LocalOpKind::MergeBase,
        SyncAllOp::RebaseBase => LocalOpKind::RebaseBase,
    }
}

fn local_op(kind: LocalOpKind) -> LocalOp {
    match kind {
        LocalOpKind::PullRebase => LocalOp::PullRebase,
        LocalOpKind::MergeRemote => LocalOp::MergeRemote,
        LocalOpKind::MergeBase => LocalOp::MergeBase,
        LocalOpKind::RebaseBase => LocalOp::RebaseBase,
    }
}

pub(crate) fn sync_run(run: api::SyncRun) -> SyncRun {
    let summary = run.summary();
    let finished = run.is_finished();
    SyncRun {
        id: run.id,
        op: local_op(run.op),
        started_at: run.started_at.into(),
        finished_at: run.finished_at.map(Into::into),
        progress_text: summary.describe(finished),
        summary: SyncSummary {
            total: summary.total,
            done: summary.done,
            updated: summary.updated,
            up_to_date: summary.up_to_date,
            handed_off: summary.handed_off,
            conflicts: summary.conflicts,
            refused: summary.refused,
            failed: summary.failed,
            skipped: summary.skipped,
        },
        entries: run
            .entries
            .into_iter()
            .map(|entry| SyncEntry {
                repo: entry.key.repo.to_string(),
                number: entry.key.number.0,
                head_ref: entry.head_ref,
                state: match entry.state {
                    api::SyncEntryState::Pending => SyncEntryState::Pending,
                    api::SyncEntryState::Running => SyncEntryState::Running,
                    api::SyncEntryState::Done { outcome } => SyncEntryState::Done {
                        result: job_result(outcome),
                    },
                },
            })
            .collect(),
    }
}

pub(crate) fn handoff(session: api::HandoffSession) -> HandoffSession {
    HandoffSession {
        attach_command: session.attach_command(),
        repo: session.key.as_ref().map(|key| key.repo.to_string()),
        number: session.key.as_ref().map(|key| key.number.0),
        session: session.session,
        head_ref: session.head_ref,
        worktree: session.worktree,
        started_at: session.started_at.map(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rostrum_core::{PrNumber, RepoId};
    use rostrum_remote::{
        CertFingerprint, Endpoint, PairingCode,
        api::{LocalBranchStatus, PrKey},
    };

    use super::*;

    fn key(n: u32) -> PrKey {
        PrKey {
            repo: RepoId::new("a", "b"),
            number: PrNumber(n),
        }
    }

    #[test]
    fn a_preview_shows_what_to_compare() {
        let offer = PairingOffer {
            machine: "desk".into(),
            endpoint: Endpoint::new(
                vec!["192.168.1.20".parse().expect("host"), "desk.ts.net".parse().expect("host")],
                8485,
                CertFingerprint::from_bytes([0x4F; 32]),
            )
            .expect("endpoint"),
            code: PairingCode::parse("k7qxm2pd").expect("code"),
        };
        let preview = preview(&offer);
        assert_eq!(preview.hosts, vec!["192.168.1.20", "desk.ts.net"]);
        assert_eq!(preview.code, "K7QX-M2PD");
        assert_eq!(preview.fingerprint_short, "4F4F · 4F4F · 4F4F");
        assert_eq!(preview.port, 8485);
    }

    #[test]
    fn a_checked_out_branch_carries_its_state() {
        let status = local_status(api::LocalStatus::CheckedOut {
            branch: LocalBranchStatus {
                worktree: "/w".into(),
                branch: "topic".into(),
                ahead: 1,
                behind: 2,
                fetched: false,
                blocker: Some("dirty".into()),
                in_progress: Some(WireInProgress::CherryPick),
                handoff: Some(api::HandoffStatus::Gone {
                    session: "rostrum-a-b-3".into(),
                }),
            },
        });
        let LocalStatus::CheckedOut { branch } = status else {
            panic!("checked out");
        };
        assert_eq!((branch.ahead, branch.behind, branch.fetched), (1, 2, false));
        let in_progress = branch.in_progress.expect("in progress");
        assert_eq!(in_progress.kind, InProgressKind::CherryPick);
        assert!(!in_progress.abortable);
        let handoff = branch.handoff.expect("handoff");
        assert!(!handoff.running);
        assert_eq!(handoff.attach_command, "tmux attach -t =rostrum-a-b-3");
    }

    #[test]
    fn job_results_carry_the_desktops_chip_colours() {
        let handed = job_result(api::JobOutcome::HandedOff {
            session: "s".into(),
        });
        assert_eq!(handed.chip.as_ref().map(|c| c.role), Some(ColorRole::Accent));
        assert_eq!(
            handed.outcome,
            JobOutcome::HandedOff {
                session: "s".into(),
                attach_command: "tmux attach -t =s".into()
            }
        );
        let conflicted = job_result(api::JobOutcome::Conflicted {
            reason: "conflict in a.rs".into(),
        });
        assert_eq!(conflicted.chip.as_ref().map(|c| c.role), Some(ColorRole::Danger));
        assert_eq!(conflicted.detail, "conflict in a.rs");
        let refused = job_result(api::JobOutcome::Refused {
            reason: "dirty".into(),
        });
        assert_eq!(refused.chip.map(|c| c.role), Some(ColorRole::Warning));
        let done = job_result(api::JobOutcome::Completed);
        assert!(done.chip.is_none());
        assert_eq!(done.detail, "updated");
    }

    #[test]
    fn a_sync_run_reports_progress_then_a_summary() {
        let mut run = api::SyncRun {
            id: 4,
            op: LocalOpKind::RebaseBase,
            started_at: Utc::now(),
            finished_at: None,
            entries: vec![
                api::SyncEntry {
                    key: key(1),
                    head_ref: "one".into(),
                    state: api::SyncEntryState::Done {
                        outcome: api::JobOutcome::Completed,
                    },
                },
                api::SyncEntry {
                    key: key(2),
                    head_ref: "two".into(),
                    state: api::SyncEntryState::Running,
                },
            ],
        };
        let running = sync_run(run.clone());
        assert_eq!(running.op, LocalOp::RebaseBase);
        assert_eq!(running.progress_text, "1/2\u{2026}");
        assert_eq!(running.entries[1].state, SyncEntryState::Running);

        run.finished_at = Some(Utc::now());
        run.entries[1].state = api::SyncEntryState::Done {
            outcome: api::JobOutcome::NotCheckedOut,
        };
        let finished = sync_run(run);
        assert_eq!(finished.progress_text, "1 updated, 1 not checked out");
        assert_eq!(finished.summary.skipped, 1);
    }

    #[test]
    fn ops_map_onto_the_protocol() {
        assert_eq!(op_kind(LocalOp::MergeRemote), LocalOpKind::MergeRemote);
        assert_eq!(sync_op_kind(SyncAllOp::Pull), LocalOpKind::PullRebase);
        assert_eq!(local_op(LocalOpKind::MergeBase), LocalOp::MergeBase);
    }

    #[test]
    fn handoff_sessions_carry_their_pull_request() {
        let session = handoff(api::HandoffSession {
            session: "s1".into(),
            key: Some(key(9)),
            head_ref: Some("topic".into()),
            worktree: None,
            started_at: None,
        });
        assert_eq!(session.repo.as_deref(), Some("a/b"));
        assert_eq!(session.number, Some(9));
        assert_eq!(session.attach_command, "tmux attach -t =s1");
    }
}
