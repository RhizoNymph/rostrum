//! Stack jobs the coordinator remembers, for a phone to poll.
//!
//! Pure bookkeeping, owned by the coordinator task: ids are handed out in
//! order, a status moves from `Running` to a finished state once, and only
//! the most recent [`KEEP`] jobs are kept — a phone polls a job for minutes,
//! not days. Exclusion is not here: a stack job holds its clone's lease like
//! any other job.

use std::collections::VecDeque;

use chrono::Utc;
use rostrum_core::RepoId;
use rostrum_remote::{StackJobId, StackJobKind, StackJobState, StackJobStatus};

/// How many jobs are remembered.
pub const KEEP: usize = 32;

#[derive(Debug)]
pub struct StackJobBook {
    next: u64,
    jobs: VecDeque<StackJobStatus>,
}

impl Default for StackJobBook {
    fn default() -> Self {
        Self {
            next: 1,
            jobs: VecDeque::new(),
        }
    }
}

impl StackJobBook {
    /// Record a job that is starting.
    pub fn start(&mut self, repo: RepoId, kind: StackJobKind) -> StackJobStatus {
        let status = StackJobStatus {
            id: StackJobId(self.next),
            repo,
            kind,
            started_at: Utc::now(),
            finished_at: None,
            state: StackJobState::Running { progress: None },
        };
        self.next += 1;
        if self.jobs.len() == KEEP {
            // Evict the oldest finished job; with none finished, the oldest.
            let ix = self
                .jobs
                .iter()
                .position(StackJobStatus::is_finished)
                .unwrap_or(0);
            self.jobs.remove(ix);
        }
        self.jobs.push_back(status.clone());
        status
    }

    /// Move job `id` on. A finished job stays finished: a late progress
    /// message cannot reopen it.
    pub fn update(&mut self, id: StackJobId, state: StackJobState) {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return;
        };
        if job.is_finished() {
            return;
        }
        if !matches!(state, StackJobState::Running { .. }) {
            job.finished_at = Some(Utc::now());
        }
        job.state = state;
    }

    pub fn get(&self, id: StackJobId) -> Option<&StackJobStatus> {
        self.jobs.iter().find(|job| job.id == id)
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::StackNumber;
    use rostrum_remote::StackJobResult;

    use super::*;

    fn repo() -> RepoId {
        RepoId::new("o", "r")
    }

    fn done() -> StackJobState {
        StackJobState::Done {
            result: StackJobResult::Unstacked {
                stack: StackNumber::new(1).expect("non-zero"),
            },
            detail: "d".into(),
        }
    }

    #[test]
    fn a_job_runs_then_finishes_once() {
        let mut book = StackJobBook::default();
        let first = book.start(repo(), StackJobKind::Unstack);
        let second = book.start(repo(), StackJobKind::Merge);
        assert_eq!((first.id, second.id), (StackJobId(1), StackJobId(2)));
        assert!(!first.is_finished());

        book.update(
            first.id,
            StackJobState::Running {
                progress: Some("Unstacking on GitHub…".into()),
            },
        );
        assert_eq!(
            book.get(first.id).expect("known").state,
            StackJobState::Running {
                progress: Some("Unstacking on GitHub…".into())
            }
        );
        assert_eq!(book.get(first.id).expect("known").finished_at, None);

        book.update(first.id, done());
        let finished = book.get(first.id).expect("known");
        assert!(finished.is_finished());
        assert!(finished.finished_at.is_some());

        // A straggling progress message does not reopen it.
        book.update(first.id, StackJobState::Running { progress: None });
        assert_eq!(book.get(first.id).expect("known").state, done());
        assert!(book.get(StackJobId(99)).is_none());
    }

    #[test]
    fn only_recent_jobs_are_kept_and_finished_ones_go_first() {
        let mut book = StackJobBook::default();
        let running = book.start(repo(), StackJobKind::Arrange);
        let mut ids = vec![];
        for _ in 1..KEEP {
            let job = book.start(repo(), StackJobKind::Unstack);
            book.update(job.id, done());
            ids.push(job.id);
        }
        let newest = book.start(repo(), StackJobKind::Merge);
        assert!(book.get(running.id).is_some(), "the running job is kept");
        assert!(book.get(ids[0]).is_none(), "the oldest finished one went");
        assert!(book.get(newest.id).is_some());
    }
}
