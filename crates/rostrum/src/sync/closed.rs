//! The store's closed half: fetching each repository's recently merged and
//! closed pull requests for the Closed tab.
//!
//! Closed pull requests are history, not work waiting on anyone, so they
//! are fetched only while someone is looking: when the Closed tab is shown
//! (for every repository whose list is missing or older than the poll
//! interval), and on each poll while it stays shown. With the tab unused
//! they cost nothing. Each fetch is one request per repository, guarded
//! against overlap like the open lists, and its result is cached so the tab
//! paints on a cold start.

use chrono::Utc;
use gpui::Context;
use gpui_tokio::Tokio;
use rostrum_core::{FeedTab, LoadState, RepoId, closed_needs_fetch};
use rostrum_github::{CLOSED_PER_REPO, GitHubError, RepoClosedPulls};

use super::Store;

impl Store {
    /// Fetch one repository's closed pull requests, unless a fetch is
    /// already in flight for it.
    pub fn refresh_closed(&mut self, id: RepoId, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        if self.pending_closed.contains_key(&id) {
            return;
        }
        let Some(repo) = self.state.repo_mut(&id) else {
            return;
        };
        // Only a first load says "loading": a background refresh must not
        // turn a list that is showing into a notice.
        if repo.closed.is_empty() {
            repo.closed_load = LoadState::Loading;
        }
        cx.notify();

        let fetch_id = id.clone();
        let apply_id = id.clone();
        let task = cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(&*cx, async move {
                client
                    .closed_pull_requests(&fetch_id, CLOSED_PER_REPO)
                    .await
            })
            .await;
            // No await points after this: `apply_closed_refresh` removes this
            // task's own handle from `pending_closed`, which drops it.
            this.update(cx, |this, cx| {
                this.apply_closed_refresh(&apply_id, outcome, cx)
            })
            .ok();
        });
        self.pending_closed.insert(id, task);
    }

    /// While the Closed tab is showing, fetch every repository whose closed
    /// list is missing, failed, or older than the poll interval. Called when
    /// the tab is shown and whenever the set of repositories changes, so
    /// switching back and forth does not refetch a fresh list.
    pub(crate) fn refresh_closed_if_shown(&mut self, cx: &mut Context<Self>) {
        if self.state.tab != FeedTab::Closed {
            return;
        }
        let now = Utc::now();
        let max_age = chrono::Duration::from_std(self.config.refresh_interval())
            .unwrap_or(chrono::Duration::MAX);
        let due: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .filter(|repo| closed_needs_fetch(&repo.closed_load, now, max_age))
            .map(|repo| repo.id.clone())
            .collect();
        if !due.is_empty() {
            tracing::debug!(repos = due.len(), "fetching closed pull requests");
        }
        for id in due {
            self.refresh_closed(id, cx);
        }
    }

    fn apply_closed_refresh(
        &mut self,
        id: &RepoId,
        outcome: Result<Result<RepoClosedPulls, GitHubError>, gpui_tokio::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        let failure = match outcome {
            Ok(Ok(fetched)) => {
                tracing::debug!(
                    repo = %id,
                    count = fetched.closed.len(),
                    rejected = fetched.rejected.len(),
                    cost = fetched.rate_limit.as_ref().map(|limit| limit.cost),
                    remaining = fetched.rate_limit.as_ref().map(|limit| limit.remaining),
                    "closed pull requests refreshed"
                );
                if let Some(repo) = self.state.repo_mut(id) {
                    repo.closed = fetched.closed;
                    repo.closed_load = LoadState::Loaded { at: now };
                }
                self.cache_closed(id, cx);
                None
            }
            Ok(Err(error)) => {
                tracing::warn!(repo = %id, %error, "closed pull request refresh failed");
                Some(error.to_string())
            }
            Err(error) => {
                tracing::warn!(repo = %id, %error, "closed pull request refresh did not complete");
                Some(format!(
                    "closed pull request refresh did not complete: {error}"
                ))
            }
        };
        if let Some(message) = failure
            && let Some(repo) = self.state.repo_mut(id)
        {
            repo.closed_load = LoadState::Failed { message, at: now };
        }

        self.pending_closed.remove(id);
        cx.notify();
    }

    /// Write a repository's closed list to the cache, fire and forget: a
    /// failed cache write must not disturb the refresh that produced it.
    fn cache_closed(&self, id: &RepoId, cx: &mut Context<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let repo = id.clone();
        let closed = self
            .state
            .repo(id)
            .map(|repo| repo.closed.clone())
            .unwrap_or_default();
        Tokio::spawn(&*cx, async move {
            if let Err(error) = db.save_closed_pulls(&repo, &closed).await {
                tracing::warn!(%repo, %error, "could not cache closed pull requests");
            }
        })
        .detach();
    }
}
