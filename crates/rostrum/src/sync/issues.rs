//! The store's issue half: refreshing the open issues of every repository
//! in batches, on their own cadence, and the feed tab.
//!
//! Issues are a separate request from pull requests rather than a second
//! selection on the pull request query. The two lists then carry separate
//! load states — one can fail while the other lands — the merge probes'
//! re-queries do not drag the issue list along, and issues can be polled
//! less often ([`rostrum_core::poll::issues_due`]): every
//! `issue_refresh_secs` (five minutes by default) rather than every poll,
//! at once on the Refresh action, and when the Issues tab is shown stale.

use chrono::{DateTime, Utc};
use gpui::Context;
use gpui_tokio::Tokio;
use rostrum_core::{
    FeedTab, Issue, LoadState, RepoId, Selection,
    budget::Feature,
    poll::{Trigger, issues_due},
};
use rostrum_github::{
    GitHubError, RepoBatch,
    batch::{BatchKind, chunk_size, chunks},
};

use super::{Store, inflight::BatchId, is_rate_limit};

type IssuesOutcome = Result<Result<RepoBatch<Vec<Issue>>, GitHubError>, gpui_tokio::JoinError>;

impl Store {
    /// Fetch one repository's open issues, unless a fetch is already in
    /// flight for it.
    pub fn refresh_issues(&mut self, id: RepoId, cx: &mut Context<Self>) {
        self.refresh_issues_batch(vec![id], cx);
    }

    /// Fetch the open issues of `ids`, leaving out any already in flight, in
    /// as many batches as [`chunk_size`] says is cheapest.
    pub(super) fn refresh_issues_batch(&mut self, ids: Vec<RepoId>, cx: &mut Context<Self>) {
        let Some(client) = self.client_for(Feature::Issues) else {
            return;
        };
        if self.paused(Feature::Issues) {
            return;
        }
        let known: Vec<RepoId> = ids
            .into_iter()
            .filter(|id| self.state.repo(id).is_some())
            .collect();
        let ids = self.issues_flight.claimable(known);
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            // A spinner only on first load; a background refresh must not
            // blank a list that is already showing.
            if let Some(repo) = self.state.repo_mut(id)
                && repo.issues.is_empty()
            {
                repo.issues_load = LoadState::Loading;
            }
        }
        cx.notify();

        let limit = self.config.issues_per_repo;
        let size = chunk_size(BatchKind::Issues, limit, ids.len());
        for range in chunks(ids.len(), size) {
            let chunk = ids[range].to_vec();
            let batch = self.issues_flight.next_id();
            let client = client.clone();
            let fetch = chunk.clone();
            let apply = chunk.clone();
            let task = cx.spawn(async move |this, cx| {
                let outcome = Tokio::spawn(&*cx, async move {
                    client.open_issues_batch(&fetch, limit).await
                })
                .await;
                // No await points after this: `apply_issue_batch` releases
                // this task's own handle.
                this.update(cx, |this, cx| {
                    this.apply_issue_batch(batch, apply, outcome, cx)
                })
                .ok();
            });
            self.issues_flight.start(batch, chunk, task);
        }
    }

    fn apply_issue_batch(
        &mut self,
        batch: BatchId,
        chunk: Vec<RepoId>,
        outcome: IssuesOutcome,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        match outcome {
            Ok(Ok(answer)) => {
                tracing::debug!(
                    repos = chunk.len(),
                    cost = answer.rate_limit.as_ref().map(|limit| limit.cost),
                    remaining = answer.rate_limit.as_ref().map(|limit| limit.remaining),
                    "issues refreshed"
                );
                for (id, result) in answer.results {
                    match result {
                        Ok(issues) => {
                            if let Some(repo) = self.state.repo_mut(&id) {
                                repo.issues = issues;
                                repo.issues_load = LoadState::Loaded { at: now };
                            }
                            self.cache_issues(&id, cx);
                        }
                        Err(error) => self.issues_failed(&id, &error, now),
                    }
                }
            }
            Ok(Err(error)) => {
                for id in &chunk {
                    self.issues_failed(id, &error, now);
                }
            }
            Err(error) => {
                tracing::warn!(repos = chunk.len(), %error, "issue refresh did not complete");
                for id in &chunk {
                    if let Some(repo) = self.state.repo_mut(id) {
                        repo.issues_load = LoadState::Failed {
                            message: format!("issue refresh did not complete: {error}"),
                            at: now,
                        };
                    }
                }
            }
        }
        self.issues_flight.finish(batch);
        cx.notify();
    }

    /// One repository's issue refresh failed; a rate-limit refusal leaves a
    /// list that is showing alone, as for pull requests.
    fn issues_failed(&mut self, id: &RepoId, error: &GitHubError, now: DateTime<Utc>) {
        tracing::warn!(repo = %id, %error, "issue refresh failed");
        let Some(repo) = self.state.repo_mut(id) else {
            return;
        };
        if is_rate_limit(error) && !repo.issues.is_empty() {
            return;
        }
        repo.issues_load = LoadState::Failed {
            message: error.to_string(),
            at: now,
        };
    }

    /// Fetch every repository's issues that are older than their cadence:
    /// what showing the Issues tab does, so a tab left alone for a while is
    /// not shown stale until the next due poll.
    fn refresh_stale_issues(&mut self, cx: &mut Context<Self>) {
        let now = Utc::now();
        let cadence = self.cadence().issues;
        let due: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .filter(|repo| issues_due(&repo.issues_load, now, cadence, Trigger::Timer))
            .map(|repo| repo.id.clone())
            .collect();
        if !due.is_empty() {
            tracing::debug!(repos = due.len(), "issues tab shown stale; refreshing");
            self.refresh_issues_batch(due, cx);
        }
    }

    /// Write a repository's issues to the cache, fire and forget: a failed
    /// cache write must not disturb the refresh that produced it.
    fn cache_issues(&self, id: &RepoId, cx: &mut Context<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let repo = id.clone();
        let issues = self
            .state
            .repo(id)
            .map(|repo| repo.issues.clone())
            .unwrap_or_default();
        Tokio::spawn(&*cx, async move {
            if let Err(error) = db.save_issues(&repo, &issues).await {
                tracing::warn!(%repo, %error, "could not cache issues");
            }
        })
        .detach();
    }

    /// Show another list, and remember the choice across restarts. Showing
    /// the Closed tab fetches whatever closed lists are due.
    pub fn set_tab(&mut self, tab: FeedTab, cx: &mut Context<Self>) {
        if self.state.tab == tab {
            return;
        }
        tracing::debug!(?tab, "feed tab changed");
        self.state.tab = tab;
        self.config.feed_tab = tab;
        self.persist_config(cx);
        self.refresh_closed_if_shown(cx);
        if tab == FeedTab::Issues {
            self.refresh_stale_issues(cx);
        }
        cx.notify();
    }

    /// Point the detail pane at `selection` and show the tab it lives on, so
    /// its row is in view. Used once an issue has been created from the feed.
    pub fn reveal(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let tab = selection.tab();
        self.state.selection = Some(selection);
        self.set_tab(tab, cx);
        cx.notify();
    }
}
