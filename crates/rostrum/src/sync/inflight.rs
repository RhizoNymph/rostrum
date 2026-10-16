//! The overlap guard for batched fetches.
//!
//! One request now carries several repositories, so "a fetch is in flight
//! for this repository" is a set membership and the tasks are held per
//! batch. A repository already in flight is left out of a new batch rather
//! than fetched twice; dropping a batch's task cancels its request.

use std::collections::{BTreeMap, BTreeSet};

use gpui::Task;
use rostrum_core::RepoId;

/// Identifies one batch among those a [`InFlight`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BatchId(u64);

#[derive(Default)]
pub struct InFlight {
    /// Every repository some held batch is fetching.
    repos: BTreeSet<RepoId>,
    /// Each batch's task and the repositories it covers.
    batches: BTreeMap<BatchId, (Vec<RepoId>, Task<()>)>,
    next: u64,
}

impl InFlight {
    /// The repositories of `wanted` not already in flight, in order, without
    /// duplicates.
    pub fn claimable(&self, wanted: impl IntoIterator<Item = RepoId>) -> Vec<RepoId> {
        let mut seen = BTreeSet::new();
        wanted
            .into_iter()
            .filter(|id| !self.repos.contains(id) && seen.insert(id.clone()))
            .collect()
    }

    /// A fresh id for a batch about to start.
    pub fn next_id(&mut self) -> BatchId {
        self.next += 1;
        BatchId(self.next)
    }

    /// Hold `task`, the batch `id` over `repos`.
    pub fn start(&mut self, id: BatchId, repos: Vec<RepoId>, task: Task<()>) {
        self.repos.extend(repos.iter().cloned());
        self.batches.insert(id, (repos, task));
    }

    /// Batch `id` has applied its results; release its repositories.
    ///
    /// Called from inside that batch's own task, as its last step: dropping
    /// a task's handle from its final update is the same pattern the
    /// one-repository guards used, and nothing awaits after it.
    pub fn finish(&mut self, id: BatchId) {
        if let Some((repos, _task)) = self.batches.remove(&id) {
            for repo in repos {
                self.repos.remove(&repo);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// Stop tracking a repository that is no longer watched. Its batch keeps
    /// running for the others; its result finds no repository to land on.
    pub fn forget(&mut self, id: &RepoId) {
        self.repos.remove(id);
    }
}
