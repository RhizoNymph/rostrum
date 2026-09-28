//! Which clone is busy, the sync-all run, and the handoff record — owned by
//! one coordinator task.
//!
//! The rules:
//!
//! - **One thing per clone.** A status read, a job or an abort holds its
//!   clone's [`Lease`] for as long as it runs. A second request on the same
//!   clone is refused with [`Busy`] rather than queued: a phone that
//!   double-taps should not get a second rebase minutes later against a
//!   repository that has changed since.
//! - **Sync-all owns what it touches.** Starting a run reserves every clone it
//!   will visit; each is released after the run's last entry on it. Nothing
//!   else runs on a reserved clone, and a second run cannot start while one is
//!   going.
//! - **Work outlives the request.** Everything that touches git runs in a
//!   spawned task. A phone that drops its connection mid-rebase must not drop
//!   the rebase (git would be killed half-way), so the request only awaits the
//!   task's answer and the lease is released when the work ends, not when the
//!   request does.
//!
//! "A clone" is a repository, identified by its common git directory: two
//! worktrees of one repository are one clone.

mod actor;
pub mod handoffs;
pub mod sync;

use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

use rostrum_git::Repo;
use rostrum_local::{LocalJob, LocalResult};
use rostrum_remote::{PrKey, SyncRun};
use tokio::sync::{mpsc, oneshot};

pub use handoffs::{HandoffBook, HandoffRecord};
pub use sync::{EntryTarget, PlannedEntry, SyncPlan};

use crate::boxed::BoxFuture;

/// Runs one local job to completion. The daemon uses
/// [`rostrum_local::run_local_job`]; tests substitute one that records calls.
pub type JobRunner = Arc<dyn Fn(LocalJob) -> BoxFuture<'static, LocalResult> + Send + Sync>;

/// The real thing.
pub fn live_runner() -> JobRunner {
    Arc::new(|job| Box::pin(rostrum_local::run_local_job(job)))
}

/// A repository's identity for mutual exclusion: its common git directory,
/// canonicalised.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CloneKey(PathBuf);

impl CloneKey {
    /// The common git directory of the repository `clone` is a worktree of.
    /// A path git cannot open is keyed by itself; the work on it will fail
    /// with git's own reason.
    pub async fn resolve(clone: &Path) -> Self {
        match Repo::open(clone).await {
            Ok(repo) => Self::of_path(repo.common_dir()),
            Err(_) => Self::of_path(clone),
        }
    }

    /// Key by a path directly, canonicalised when it exists.
    pub fn of_path(path: &Path) -> Self {
        Self(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// Why a request on a clone was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Busy {
    #[error("another job is already running on this clone; try again when it finishes")]
    Job,
    #[error("a sync-all run is working through this clone; try again when it finishes")]
    SyncAll,
    #[error("a sync-all run is already in progress")]
    SyncRunning,
    #[error("rostrumd is shutting down")]
    ShuttingDown,
}

#[derive(Debug, thiserror::Error)]
pub enum JobsError {
    #[error(transparent)]
    Busy(#[from] Busy),
    #[error("the job stopped unexpectedly: {0}")]
    Crashed(String),
    #[error("the job coordinator has stopped")]
    Stopped,
}

/// Messages to the coordinator. Unbounded so a [`Lease`] can release itself
/// from `Drop`, which cannot wait.
enum Command {
    Acquire {
        key: CloneKey,
        reply: oneshot::Sender<Result<Lease, Busy>>,
    },
    Release {
        key: CloneKey,
    },
    StartSync {
        plan: SyncPlan,
        reply: oneshot::Sender<Result<SyncRun, Busy>>,
    },
    SyncProgress {
        run: u64,
        index: usize,
        state: rostrum_remote::SyncEntryState,
    },
    SyncFinished {
        run: u64,
    },
    LatestSync {
        reply: oneshot::Sender<Option<SyncRun>>,
    },
    RecordHandoff {
        record: HandoffRecord,
    },
    Handoffs {
        reply: oneshot::Sender<Vec<HandoffRecord>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

/// Exclusive use of one clone, released on drop.
#[derive(Debug)]
pub struct Lease {
    key: CloneKey,
    tx: mpsc::UnboundedSender<Command>,
}

impl Lease {
    pub fn key(&self) -> &CloneKey {
        &self.key
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Release {
            key: self.key.clone(),
        });
    }
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Acquire { .. } => "Acquire",
            Self::Release { .. } => "Release",
            Self::StartSync { .. } => "StartSync",
            Self::SyncProgress { .. } => "SyncProgress",
            Self::SyncFinished { .. } => "SyncFinished",
            Self::LatestSync { .. } => "LatestSync",
            Self::RecordHandoff { .. } => "RecordHandoff",
            Self::Handoffs { .. } => "Handoffs",
            Self::Shutdown { .. } => "Shutdown",
        })
    }
}

/// A handle to the coordinator. Cheap to clone.
#[derive(Clone)]
pub struct Jobs {
    tx: mpsc::UnboundedSender<Command>,
    runner: JobRunner,
}

impl Jobs {
    /// Start the coordinator. Must be called inside a Tokio runtime.
    pub fn spawn(runner: JobRunner, handoffs: HandoffBook) -> Self {
        let tx = actor::spawn(runner.clone(), handoffs);
        Self { tx, runner }
    }

    async fn ask<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, JobsError> {
        let (reply, answer) = oneshot::channel();
        self.tx.send(make(reply)).map_err(|_| JobsError::Stopped)?;
        answer.await.map_err(|_| JobsError::Stopped)
    }

    /// Take `key` for exclusive use, or say why not.
    pub async fn acquire(&self, key: CloneKey) -> Result<Lease, JobsError> {
        Ok(self.ask(|reply| Command::Acquire { key, reply }).await??)
    }

    /// Run `work` holding `key`, in its own task, and wait for its answer.
    pub async fn exclusive<T, F>(&self, key: CloneKey, work: F) -> Result<T, JobsError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let lease = self.acquire(key).await?;
        let task = tokio::spawn(async move {
            let out = work.await;
            drop(lease);
            out
        });
        task.await
            .map_err(|err| JobsError::Crashed(err.to_string()))
    }

    /// Run one local job holding its clone, recording a handoff if there is
    /// one.
    pub async fn run_job(
        &self,
        key: CloneKey,
        job: LocalJob,
        pr: PrKey,
    ) -> Result<LocalResult, JobsError> {
        let runner = self.runner.clone();
        let tx = self.tx.clone();
        self.exclusive(key, async move {
            let clone = job.clone.clone();
            let branch = job.branch.clone();
            let head_ref = job.pr.head_ref.clone();
            let result = runner(job).await;
            if let LocalResult::HandedOff { session } = &result {
                let record =
                    HandoffRecord::describe(session.clone(), pr, head_ref, &clone, &branch).await;
                let _ = tx.send(Command::RecordHandoff { record });
            }
            result
        })
        .await
    }

    /// Start a sync-all run; answers with the run as it starts.
    pub async fn start_sync(&self, plan: SyncPlan) -> Result<SyncRun, JobsError> {
        Ok(self
            .ask(|reply| Command::StartSync { plan, reply })
            .await??)
    }

    /// The latest run, running or finished.
    pub async fn latest_sync(&self) -> Result<Option<SyncRun>, JobsError> {
        self.ask(|reply| Command::LatestSync { reply }).await
    }

    pub async fn handoff_records(&self) -> Result<Vec<HandoffRecord>, JobsError> {
        self.ask(|reply| Command::Handoffs { reply }).await
    }

    /// Refuse new work, stop any sync-all run between entries, and wait until
    /// the work already running has released its clones.
    pub async fn shutdown(&self) {
        let _ = self.ask(|reply| Command::Shutdown { reply }).await;
    }
}

#[cfg(test)]
mod tests;
