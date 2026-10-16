//! The store's CI half: the checks of every repository whose checks could
//! have changed, fetched in batches after the feed's poll lands, faster while
//! the CI view is open with something running, and the optimistic flip a
//! re-run makes.
//!
//! What "could have changed" means is `rostrum_core::poll::ci_due`: a head
//! moved, a pull request was added, something was queued or running, the
//! last fetch failed or never happened, or ten minutes passed.

use std::{collections::BTreeSet, time::Duration};

use chrono::Utc;
use gpui::Context;
use gpui_tokio::Tokio;
use rostrum_core::{
    PrNumber, RepoId,
    budget::Feature,
    ci::{PrChecks, RerunTarget, mark_requeued},
    poll::{Trigger, plan_ci, running_repos},
};
use rostrum_github::{
    GitHubError, RepoBatch,
    batch::{BatchKind, chunk_size, chunks},
};

use super::{Store, inflight::BatchId};

/// How often the CI view re-checks while anything is queued or running.
pub const CI_WATCH_INTERVAL: Duration = Duration::from_secs(15);

type CiOutcome = Result<Result<RepoBatch<Vec<PrChecks>>, GitHubError>, gpui_tokio::JoinError>;

impl Store {
    /// Fetch one repository's checks now, unless a fetch is already in
    /// flight: what a re-run reconciles with.
    pub fn refresh_ci(&mut self, id: RepoId, cx: &mut Context<Self>) {
        self.refresh_ci_batch(vec![id], cx);
    }

    /// Fetch the checks of every repository (or of `only`) that
    /// [`plan_ci`] says could have changed.
    pub(super) fn refresh_ci_planned(
        &mut self,
        only: Option<&BTreeSet<RepoId>>,
        trigger: Trigger,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        let plan = plan_ci(
            &self.state.repos,
            &self.ci,
            now,
            self.cadence().ci_safety,
            trigger,
            only,
        );
        let considered = only.map_or(self.state.repos.len(), BTreeSet::len);
        tracing::debug!(
            due = plan.len(),
            reused = considered.saturating_sub(plan.len()),
            reasons = ?plan.iter().map(|(id, reason)| format!("{id}: {}", reason.label())).collect::<Vec<_>>(),
            "checks planned"
        );
        self.refresh_ci_batch(plan.into_iter().map(|(id, _)| id).collect(), cx);
    }

    /// Fetch the checks of `ids`, leaving out any already in flight, in as
    /// many batches as [`chunk_size`] says is cheapest.
    pub(super) fn refresh_ci_batch(&mut self, ids: Vec<RepoId>, cx: &mut Context<Self>) {
        let Some(client) = self.client_for(Feature::Ci) else {
            return;
        };
        if self.paused(Feature::Ci) {
            return;
        }
        let known: Vec<RepoId> = ids
            .into_iter()
            .filter(|id| self.state.repo(id).is_some())
            .collect();
        let ids = self.ci_flight.claimable(known);
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            self.ci.begin(id);
        }
        cx.notify();

        let limit = self.config.prs_per_repo;
        let size = chunk_size(BatchKind::Ci, limit, ids.len());
        for range in chunks(ids.len(), size) {
            let chunk = ids[range].to_vec();
            let batch = self.ci_flight.next_id();
            let client = client.clone();
            let fetch = chunk.clone();
            let apply = chunk.clone();
            let task = cx.spawn(async move |this, cx| {
                let outcome =
                    Tokio::spawn(
                        &*cx,
                        async move { client.ci_checks_batch(&fetch, limit).await },
                    )
                    .await;
                // No await points after this: `apply_ci_batch` releases this
                // task's own handle.
                this.update(cx, |this, cx| {
                    this.apply_ci_batch(batch, apply, outcome, cx)
                })
                .ok();
            });
            self.ci_flight.start(batch, chunk, task);
        }
    }

    fn apply_ci_batch(
        &mut self,
        batch: BatchId,
        chunk: Vec<RepoId>,
        outcome: CiOutcome,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        match outcome {
            Ok(Ok(answer)) => {
                tracing::debug!(
                    repos = chunk.len(),
                    cost = answer.rate_limit.as_ref().map(|limit| limit.cost),
                    remaining = answer.rate_limit.as_ref().map(|limit| limit.remaining),
                    "checks refreshed"
                );
                for (id, result) in answer.results {
                    match result {
                        Ok(checks) => {
                            tracing::debug!(
                                repo = %id,
                                prs = checks.len(),
                                checks = checks.iter().map(|pr| pr.entries.len()).sum::<usize>(),
                                "checks loaded"
                            );
                            self.ci.loaded(&id, checks, now);
                        }
                        Err(error) => {
                            tracing::warn!(repo = %id, %error, "checks refresh failed");
                            self.ci.failed(&id, error.to_string(), now);
                        }
                    }
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(repos = chunk.len(), %error, "checks refresh failed");
                for id in &chunk {
                    self.ci.failed(id, error.to_string(), now);
                }
            }
            Err(error) => {
                tracing::warn!(repos = chunk.len(), %error, "checks refresh did not complete");
                for id in &chunk {
                    self.ci
                        .failed(id, format!("checks refresh did not complete: {error}"), now);
                }
            }
        }
        self.ci_flight.finish(batch);
        cx.notify();
    }

    /// The CI view opened or closed. Opening fetches whatever could have
    /// changed; while it is open, repositories with anything queued or
    /// running are re-checked every [`CI_WATCH_INTERVAL`] in one batch
    /// instead of waiting for the feed's poll; closing it drops the timer.
    /// The watch stops with every other fetch while GitHub has paused
    /// polling.
    pub fn set_ci_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if !visible {
            self.ci_watch = None;
            return;
        }
        if self.ci_watch.is_some() {
            return;
        }
        self.refresh_ci_planned(None, Trigger::Timer, cx);
        self.ci_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CI_WATCH_INTERVAL).await;
                let alive = this.update(cx, |this, cx| {
                    let running = running_repos(&this.state.repos, &this.ci);
                    if !running.is_empty() {
                        this.refresh_ci_batch(running, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    /// Show what a re-run restarts as queued until the next fetch reconciles
    /// it — with the new attempt, or the old result if the request failed.
    pub fn requeue(
        &mut self,
        repo: &RepoId,
        number: PrNumber,
        target: RerunTarget,
        cx: &mut Context<Self>,
    ) {
        if let Some(checks) = self.ci.pr_mut(repo, number) {
            let marked = mark_requeued(checks, target);
            tracing::debug!(%repo, number = number.0, ?target, marked, "re-run shown as queued");
        }
        cx.notify();
    }
}
