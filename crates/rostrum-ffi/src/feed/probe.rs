//! Re-checks of merge states GitHub is still computing: one probe per
//! cycle, over every repository still computing, in one feed batch.
//!
//! GitHub computes `mergeable` and `mergeStateStatus` lazily: the query that
//! asks for them returns `UNKNOWN` *and* starts the computation, so a single
//! refresh can never see the answer. Probes back off 2 s, 4 s, 8 s and stop
//! for the cycle after three — the steady state for a token that may not
//! read merge state. A refresh of every repository starts a new cycle.

use std::time::Duration;

use rostrum_core::{MergeProbeBudget, RepoId, budget::Feature, needs_merge_probe};
use rostrum_github::GitHubClient;
use tokio::task::AbortHandle;

use crate::{
    engine::{actor::WeakActor, state::CoreState},
    feed::land::land_pulls,
};

/// This cycle's probe allowance, and the probe scheduled or running.
#[derive(Default)]
pub(crate) struct ProbeCycle {
    budget: MergeProbeBudget,
    task: Option<AbortHandle>,
}

impl ProbeCycle {
    /// A probe is scheduled or running.
    pub(crate) fn is_pending(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    /// A new cycle: the allowance back, and any probe in flight dropped as
    /// redundant.
    pub(crate) fn restart(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.budget = MergeProbeBudget::default();
    }
}

impl Drop for ProbeCycle {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl CoreState {
    /// Repositories with a pull request whose merge state GitHub is still
    /// computing.
    pub(crate) fn computing_repos(&self) -> Vec<RepoId> {
        self.feed
            .repos
            .iter()
            .filter(|repo| needs_merge_probe(&repo.prs))
            .map(|repo| repo.id.clone())
            .collect()
    }

    /// Schedule the cycle's next probe if anything is still computing, none
    /// is pending, and the allowance is not spent.
    pub(crate) fn schedule_probe(&mut self, client: &GitHubClient, limit: u32) {
        if self.probe.is_pending() {
            return;
        }
        let computing = self.computing_repos();
        if computing.is_empty() {
            return;
        }
        let Some(delay) = self.probe.budget.next_delay() else {
            tracing::debug!(
                repos = computing.len(),
                "merge state still unknown; waiting for the next refresh"
            );
            return;
        };
        tracing::debug!(
            repos = computing.len(),
            attempt = self.probe.budget.spent(),
            delay_ms = delay.as_millis(),
            "merge state computing; scheduling a probe"
        );
        let task = tokio::spawn(probe(
            self.me.clone(),
            client.for_feature(Feature::Probes),
            limit,
            delay,
        ));
        self.probe.task = Some(task.abort_handle());
    }
}

/// One probe: wait, re-query every repository still computing (unless
/// GitHub has paused polling), apply, and schedule the next. Holds no strong
/// handle on the core while it waits or fetches, so it dies with the core.
async fn probe(me: WeakActor, client: GitHubClient, limit: u32, delay: Duration) {
    tokio::time::sleep(delay).await;
    let begun = me
        .call(|state| {
            if state.paused_for(Feature::Probes).is_some() {
                return Vec::new();
            }
            state
                .computing_repos()
                .into_iter()
                .filter_map(|repo| state.feed.begin_fetch(&repo).map(|seq| (repo, seq)))
                .collect()
        })
        .await;
    let Ok(fetches) = begun else {
        return;
    };
    if !fetches.is_empty()
        && let Err(error) = land_pulls(&me, &client, fetches, limit).await
    {
        tracing::debug!(%error, "a merge-state probe did not land");
        return;
    }
    let _ = me
        .call(move |state| {
            // This probe is finished; only a new one counts as pending.
            state.probe.task = None;
            state.schedule_probe(&client, limit);
            state.publish();
        })
        .await;
}
