//! The coordinator task behind [`super::Jobs`].

use std::collections::HashMap;

use chrono::Utc;
use rostrum_remote::{SyncEntry, SyncEntryState, SyncRun};
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    Busy, CloneKey, Command, JobRunner, Lease,
    handoffs::HandoffBook,
    sync::{SyncPlan, run_plan},
};

/// Who holds a clone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Holder {
    Job,
    Sync { run: u64 },
}

/// The run in progress: how many of its entries still need each clone.
struct ActiveSync {
    run: u64,
    remaining: HashMap<CloneKey, usize>,
    entry_keys: Vec<Option<CloneKey>>,
}

struct Actor {
    tx: mpsc::UnboundedSender<Command>,
    runner: JobRunner,
    holders: HashMap<CloneKey, Holder>,
    active: Option<ActiveSync>,
    latest: Option<SyncRun>,
    next_run: u64,
    handoffs: HandoffBook,
    stopping: bool,
    stop: watch::Sender<bool>,
    drained: Vec<oneshot::Sender<()>>,
}

pub(super) fn spawn(runner: JobRunner, handoffs: HandoffBook) -> mpsc::UnboundedSender<Command> {
    let (tx, rx) = mpsc::unbounded_channel();
    let (stop, _) = watch::channel(false);
    let actor = Actor {
        tx: tx.clone(),
        runner,
        holders: HashMap::new(),
        active: None,
        latest: None,
        next_run: 1,
        handoffs,
        stopping: false,
        stop,
        drained: Vec::new(),
    };
    tokio::spawn(actor.run(rx));
    tx
}

impl Actor {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Command>) {
        // The actor holds a sender of its own (for the sync tasks it spawns),
        // so the channel never closes; it lives as long as the runtime.
        while let Some(command) = rx.recv().await {
            self.handle(command);
            self.check_drained();
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Acquire { key, reply } => {
                let answer = self.acquire(&key).map(|()| Lease {
                    key,
                    tx: self.tx.clone(),
                });
                // If the requester is gone the lease drops here and releases
                // itself through the channel.
                let _ = reply.send(answer);
            }
            Command::Release { key } => {
                if self.holders.get(&key) == Some(&Holder::Job) {
                    self.holders.remove(&key);
                }
            }
            Command::StartSync { plan, reply } => {
                let _ = reply.send(self.start_sync(plan));
            }
            Command::SyncProgress { run, index, state } => self.progress(run, index, state),
            Command::SyncFinished { run } => self.finish(run),
            Command::LatestSync { reply } => {
                let _ = reply.send(self.latest.clone());
            }
            Command::RecordHandoff { record } => {
                let session = record.session.clone();
                match self.handoffs.upsert(record) {
                    Ok(()) => tracing::info!(%session, "recorded a conflict handoff"),
                    Err(error) => {
                        tracing::warn!(%session, %error, "could not record a conflict handoff");
                    }
                }
            }
            Command::Handoffs { reply } => {
                let _ = reply.send(self.handoffs.records().to_vec());
            }
            Command::Shutdown { reply } => {
                self.stopping = true;
                let _ = self.stop.send(true);
                self.drained.push(reply);
            }
        }
    }

    fn acquire(&mut self, key: &CloneKey) -> Result<(), Busy> {
        if self.stopping {
            return Err(Busy::ShuttingDown);
        }
        match self.holders.get(key) {
            None => {
                self.holders.insert(key.clone(), Holder::Job);
                Ok(())
            }
            Some(Holder::Job) => Err(Busy::Job),
            Some(Holder::Sync { .. }) => Err(Busy::SyncAll),
        }
    }

    fn start_sync(&mut self, plan: SyncPlan) -> Result<SyncRun, Busy> {
        if self.stopping {
            return Err(Busy::ShuttingDown);
        }
        if self.active.is_some() {
            return Err(Busy::SyncRunning);
        }
        let entry_keys: Vec<Option<CloneKey>> = plan
            .entries
            .iter()
            .map(|entry| entry.target.key().cloned())
            .collect();
        let mut remaining: HashMap<CloneKey, usize> = HashMap::new();
        for key in entry_keys.iter().flatten() {
            *remaining.entry(key.clone()).or_default() += 1;
        }
        if remaining.keys().any(|key| self.holders.contains_key(key)) {
            return Err(Busy::Job);
        }

        let run = self.next_run;
        self.next_run += 1;
        for key in remaining.keys() {
            self.holders.insert(key.clone(), Holder::Sync { run });
        }
        let snapshot = SyncRun {
            id: run,
            op: plan.op,
            started_at: Utc::now(),
            finished_at: None,
            entries: plan
                .entries
                .iter()
                .map(|entry| SyncEntry {
                    key: entry.pr.key.clone(),
                    head_ref: entry.pr.head_ref.clone(),
                    state: SyncEntryState::Pending,
                })
                .collect(),
        };
        tracing::info!(
            run,
            op = ?plan.op,
            entries = snapshot.entries.len(),
            clones = remaining.len(),
            "starting a sync-all run"
        );
        self.latest = Some(snapshot.clone());
        self.active = Some(ActiveSync {
            run,
            remaining,
            entry_keys,
        });
        tokio::spawn(run_plan(
            run,
            plan,
            self.runner.clone(),
            self.tx.clone(),
            self.stop.subscribe(),
        ));
        Ok(snapshot)
    }

    fn progress(&mut self, run: u64, index: usize, state: SyncEntryState) {
        let done = matches!(state, SyncEntryState::Done { .. });
        if let Some(latest) = self.latest.as_mut().filter(|latest| latest.id == run)
            && let Some(entry) = latest.entries.get_mut(index)
        {
            entry.state = state;
        }
        if !done {
            return;
        }
        let Some(active) = self.active.as_mut().filter(|active| active.run == run) else {
            return;
        };
        let Some(Some(key)) = active.entry_keys.get(index).cloned() else {
            return;
        };
        let left = active.remaining.entry(key.clone()).or_default();
        *left = left.saturating_sub(1);
        if *left == 0 {
            active.remaining.remove(&key);
            if self.holders.get(&key) == Some(&Holder::Sync { run }) {
                self.holders.remove(&key);
            }
        }
    }

    fn finish(&mut self, run: u64) {
        if let Some(latest) = self.latest.as_mut().filter(|latest| latest.id == run) {
            latest.finished_at = Some(Utc::now());
            let summary = latest.summary();
            tracing::info!(
                run,
                summary = %summary.describe(true),
                "finished a sync-all run"
            );
        }
        if self.active.as_ref().is_some_and(|active| active.run == run) {
            self.active = None;
        }
        self.holders
            .retain(|_, holder| *holder != Holder::Sync { run });
    }

    fn check_drained(&mut self) {
        if self.stopping && self.holders.is_empty() {
            for waiter in self.drained.drain(..) {
                let _ = waiter.send(());
            }
        }
    }
}
