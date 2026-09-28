//! Chasing merge states GitHub has not finished computing.
//!
//! GitHub computes `mergeable` and `mergeStateStatus` lazily: the query that
//! asks for them returns `UNKNOWN` *and* starts the computation, so a single
//! poll can never see the answer. A refresh that saw
//! [`MergeStatus::Computing`] is therefore followed by a few re-queries, backing
//! off, within a budget — a token that may not read a repository's merge state
//! sees `UNKNOWN` forever, and without a bound that is an endless request loop
//! rather than a slow one.
//!
//! Shared by the desktop's store and the Android core, so both chase with the
//! same schedule.

use std::time::Duration;

use crate::model::{MergeStatus, PullRequest};

/// Delay before the first re-query; doubles with each attempt.
pub const MERGE_PROBE_DELAY: Duration = Duration::from_secs(2);

/// Re-queries per repository per poll cycle. Three attempts span 2s, 4s and
/// 8s, which covers the computation comfortably; beyond that the state is not
/// pending but withheld, and waiting harder will not reveal it.
pub const MAX_MERGE_PROBES: u8 = 3;

/// Whether any pull request is waiting on GitHub's merge computation.
///
/// Drafts never count: a draft reports [`MergeStatus::Draft`] whatever GitHub
/// is computing, so there is nothing to wait for.
pub fn needs_merge_probe(prs: &[PullRequest]) -> bool {
    prs.iter()
        .any(|pr| pr.merge_status() == MergeStatus::Computing)
}

/// One repository's probe allowance for the current poll cycle.
///
/// A full refresh starts a new cycle with a fresh budget; the probes
/// themselves spend from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeProbeBudget {
    spent: u8,
}

impl MergeProbeBudget {
    /// Spend one probe, returning how long to wait before it, or `None` once
    /// the cycle's allowance is gone.
    pub fn next_delay(&mut self) -> Option<Duration> {
        if self.spent >= MAX_MERGE_PROBES {
            return None;
        }
        self.spent += 1;
        Some(MERGE_PROBE_DELAY * 2u32.pow(u32::from(self.spent - 1)))
    }

    /// Probes spent so far this cycle.
    pub fn spent(&self) -> u8 {
        self.spent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MergeStateStatus, Mergeable};
    use crate::test_support::pull;

    #[test]
    fn the_budget_backs_off_then_runs_out() {
        let mut budget = MergeProbeBudget::default();
        assert_eq!(budget.next_delay(), Some(Duration::from_secs(2)));
        assert_eq!(budget.next_delay(), Some(Duration::from_secs(4)));
        assert_eq!(budget.next_delay(), Some(Duration::from_secs(8)));
        assert_eq!(budget.next_delay(), None);
        assert_eq!(budget.next_delay(), None);
        assert_eq!(budget.spent(), MAX_MERGE_PROBES);
    }

    #[test]
    fn only_a_computing_merge_state_needs_a_probe() {
        let mut ready = pull(1);
        ready.mergeable = Mergeable::Mergeable;
        ready.merge_state = MergeStateStatus::Clean;

        let mut computing = pull(2);
        computing.mergeable = Mergeable::Unknown;

        let mut draft = pull(3);
        draft.mergeable = Mergeable::Unknown;
        draft.is_draft = true;

        assert!(!needs_merge_probe(&[]));
        assert!(!needs_merge_probe(std::slice::from_ref(&ready)));
        assert!(needs_merge_probe(&[ready.clone(), computing]));
        assert!(!needs_merge_probe(&[ready, draft]));
    }
}
