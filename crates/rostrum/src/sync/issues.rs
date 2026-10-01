//! The store's issue half: refreshing each repository's open issues, and the
//! feed tab.
//!
//! Issues are a second request per repository rather than a second selection
//! on the pull request query. The two lists then carry separate load states —
//! one can fail while the other lands — and the pull request query, which
//! merge probes re-issue on a short timer, does not drag the issue list along
//! with it.

use chrono::Utc;
use gpui::Context;
use gpui_tokio::Tokio;
use rostrum_core::{FeedTab, LoadState, RepoId, Selection};
use rostrum_github::{GitHubError, RepoIssues};

use super::Store;

impl Store {
    /// Fetch one repository's open issues, unless a fetch is already in
    /// flight for it.
    pub fn refresh_issues(&mut self, id: RepoId, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        // Overlap guard, as for pull requests.
        if self.pending_issues.contains_key(&id) {
            return;
        }
        let Some(repo) = self.state.repo_mut(&id) else {
            return;
        };
        // A spinner only on first load; a background refresh must not blank
        // a list that is already showing.
        if repo.issues.is_empty() {
            repo.issues_load = LoadState::Loading;
        }
        cx.notify();

        let limit = self.config.issues_per_repo;
        let fetch_id = id.clone();
        let apply_id = id.clone();
        let task = cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(
                &*cx,
                async move { client.open_issues(&fetch_id, limit).await },
            )
            .await;
            // No await points after this: `apply_issue_refresh` removes this
            // task's own handle from `pending_issues`, which drops it.
            this.update(cx, |this, cx| {
                this.apply_issue_refresh(&apply_id, outcome, cx)
            })
            .ok();
        });
        self.pending_issues.insert(id, task);
    }

    fn apply_issue_refresh(
        &mut self,
        id: &RepoId,
        outcome: Result<Result<RepoIssues, GitHubError>, gpui_tokio::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();
        let failure = match outcome {
            Ok(Ok(fetched)) => {
                tracing::debug!(
                    repo = %id,
                    count = fetched.issues.len(),
                    cost = fetched.rate_limit.as_ref().map(|limit| limit.cost),
                    remaining = fetched.rate_limit.as_ref().map(|limit| limit.remaining),
                    "issues refreshed"
                );
                if let Some(repo) = self.state.repo_mut(id) {
                    repo.issues = fetched.issues;
                    repo.issues_load = LoadState::Loaded { at: now };
                }
                self.cache_issues(id, cx);
                None
            }
            Ok(Err(error)) => {
                tracing::warn!(repo = %id, %error, "issue refresh failed");
                Some(error.to_string())
            }
            Err(error) => {
                tracing::warn!(repo = %id, %error, "issue refresh did not complete");
                Some(format!("issue refresh did not complete: {error}"))
            }
        };
        if let Some(message) = failure
            && let Some(repo) = self.state.repo_mut(id)
        {
            repo.issues_load = LoadState::Failed { message, at: now };
        }

        self.pending_issues.remove(id);
        cx.notify();
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

    /// Show the other list, and remember the choice across restarts.
    pub fn set_tab(&mut self, tab: FeedTab, cx: &mut Context<Self>) {
        if self.state.tab == tab {
            return;
        }
        tracing::debug!(?tab, "feed tab changed");
        self.state.tab = tab;
        self.config.feed_tab = tab;
        self.persist_config();
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
