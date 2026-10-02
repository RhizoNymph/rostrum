//! The store's CI half: each repository's checks, refreshed on the feed's
//! poll and faster while the CI view is open with something running, and the
//! optimistic flip a re-run makes.

use std::time::Duration;

use chrono::Utc;
use gpui::Context;
use gpui_tokio::Tokio;
use rostrum_core::{
    PrNumber, RepoId,
    ci::{RerunTarget, mark_requeued},
};
use rostrum_github::{GitHubError, ci::RepoCiChecks};

use super::Store;

/// How often the CI view re-checks while anything is queued or running.
pub const CI_WATCH_INTERVAL: Duration = Duration::from_secs(15);

impl Store {
    /// Fetch one repository's checks, unless a fetch is already in flight.
    pub fn refresh_ci(&mut self, id: RepoId, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        if self.pending_ci.contains_key(&id) || self.state.repo(&id).is_none() {
            return;
        }
        self.ci.begin(&id);
        cx.notify();

        let limit = self.config.prs_per_repo;
        let (fetch_id, apply_id) = (id.clone(), id.clone());
        let task = cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(
                &*cx,
                async move { client.ci_checks(&fetch_id, limit).await },
            )
            .await;
            this.update(cx, |this, cx| this.apply_ci(&apply_id, outcome, cx))
                .ok();
        });
        self.pending_ci.insert(id, task);
    }

    fn apply_ci(
        &mut self,
        id: &RepoId,
        outcome: Result<Result<RepoCiChecks, GitHubError>, gpui_tokio::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        match outcome {
            Ok(Ok(fetched)) => {
                tracing::debug!(
                    repo = %id,
                    prs = fetched.checks.len(),
                    checks = fetched.checks.iter().map(|pr| pr.entries.len()).sum::<usize>(),
                    cost = fetched.rate_limit.as_ref().map(|limit| limit.cost),
                    "checks refreshed"
                );
                self.ci.loaded(id, fetched.checks, now);
            }
            Ok(Err(error)) => {
                tracing::warn!(repo = %id, %error, "checks refresh failed");
                self.ci.failed(id, error.to_string(), now);
            }
            Err(error) => {
                tracing::warn!(repo = %id, %error, "checks refresh did not complete");
                self.ci
                    .failed(id, format!("checks refresh did not complete: {error}"), now);
            }
        }
        self.pending_ci.remove(id);
        cx.notify();
    }

    /// The CI view opened or closed. While it is open, anything queued or
    /// running is re-checked every [`CI_WATCH_INTERVAL`] instead of waiting
    /// for the feed's poll; closing it drops the timer.
    pub fn set_ci_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if !visible {
            self.ci_watch = None;
            return;
        }
        if self.ci_watch.is_some() {
            return;
        }
        // Opening the view is the moment to be current.
        let ids: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .map(|repo| repo.id.clone())
            .collect();
        for id in ids {
            self.refresh_ci(id, cx);
        }
        self.ci_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CI_WATCH_INTERVAL).await;
                let alive = this.update(cx, |this, cx| {
                    let running: Vec<RepoId> = this
                        .state
                        .repos
                        .iter()
                        .map(|repo| repo.id.clone())
                        .filter(|id| {
                            this.ci.repo(id).is_some_and(|checks| {
                                checks.prs.values().any(|pr| {
                                    pr.latest().values().any(|entry| entry.status.is_running())
                                })
                            })
                        })
                        .collect();
                    for id in running {
                        this.refresh_ci(id, cx);
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
