//! A sync-all run: one operation across many pull requests, one at a time.
//!
//! The API resolves each [`PrRef`] against the config when the run is
//! requested, producing a [`SyncPlan`] whose entries already know their clone
//! (or that they have none). The coordinator reserves every clone the plan
//! touches, then [`run_plan`] works through the entries in order, reporting
//! each step back to the coordinator, which owns the [`SyncRun`] the phone
//! polls.

use std::path::PathBuf;

use rostrum_config::ConflictHandler;
use rostrum_git::{Autostash, BranchName};
use rostrum_local::{LocalJob, LocalResult};
use rostrum_remote::{JobOutcome, LocalOpKind, PrRef, SyncEntryState};
use tokio::sync::{mpsc, watch};

use super::{CloneKey, Command, JobRunner, handoffs::HandoffRecord};
use crate::convert;

/// Everything a run needs, resolved before it starts.
#[derive(Clone, Debug)]
pub struct SyncPlan {
    pub op: LocalOpKind,
    pub autostash: Autostash,
    pub handler: Option<ConflictHandler>,
    pub entries: Vec<PlannedEntry>,
}

#[derive(Clone, Debug)]
pub struct PlannedEntry {
    pub pr: PrRef,
    pub target: EntryTarget,
}

/// Where an entry will run, decided up front.
#[derive(Clone, Debug)]
pub enum EntryTarget {
    /// No clone is configured for the repository; skipped as `NotConfigured`.
    NotConfigured,
    /// A branch name failed validation; reported as `Failed` without touching
    /// git. One odd branch does not sink the whole run.
    Invalid { reason: String },
    Clone {
        key: CloneKey,
        path: PathBuf,
        head: BranchName,
        base: BranchName,
    },
}

impl EntryTarget {
    pub fn key(&self) -> Option<&CloneKey> {
        match self {
            Self::Clone { key, .. } => Some(key),
            Self::NotConfigured | Self::Invalid { .. } => None,
        }
    }
}

/// Work through `plan` in order, reporting each entry's progress to the
/// coordinator. Stops between entries once `stop` turns true.
pub(super) async fn run_plan(
    run: u64,
    plan: SyncPlan,
    runner: JobRunner,
    tx: mpsc::UnboundedSender<Command>,
    stop: watch::Receiver<bool>,
) {
    let SyncPlan {
        op,
        autostash,
        handler,
        entries,
    } = plan;
    for (index, entry) in entries.into_iter().enumerate() {
        if *stop.borrow() {
            tracing::info!(run, "stopping a sync-all run for shutdown");
            break;
        }
        let _ = tx.send(Command::SyncProgress {
            run,
            index,
            state: SyncEntryState::Running,
        });
        let outcome = match entry.target {
            EntryTarget::NotConfigured => JobOutcome::NotConfigured,
            EntryTarget::Invalid { reason } => JobOutcome::Failed { reason },
            EntryTarget::Clone {
                path, head, base, ..
            } => {
                let job = LocalJob {
                    clone: path.clone(),
                    branch: head.clone(),
                    base,
                    op: convert::local_op(op),
                    autostash,
                    handler: handler.clone(),
                    pr: convert::pr_meta(&entry.pr),
                };
                let result = runner(job).await;
                if let LocalResult::HandedOff { session } = &result {
                    let record = HandoffRecord::describe(
                        session.clone(),
                        entry.pr.key.clone(),
                        entry.pr.head_ref.clone(),
                        &path,
                        &head,
                    )
                    .await;
                    let _ = tx.send(Command::RecordHandoff { record });
                }
                convert::job_outcome(result)
            }
        };
        let _ = tx.send(Command::SyncProgress {
            run,
            index,
            state: SyncEntryState::Done { outcome },
        });
    }
    let _ = tx.send(Command::SyncFinished { run });
}
