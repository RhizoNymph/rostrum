//! The feed: every watched repository's open pull requests, filtered.

pub(crate) mod chips;
mod refresh;
mod state;
pub(crate) mod summary;
mod types;

use std::sync::Arc;

use rostrum_core::{LoginKey, authors::visible, issue_roster, roster};

pub(crate) use refresh::{ProbeSlot, Probes, Scope};
pub(crate) use state::{FeedState, count, load_of};
pub use types::{
    AuthorChip, AuthorRoster, BaseDivergence, FeedObserver, FeedPreferences, FeedSnapshot, FeedTab,
    PrSummary, RepoBody, RepoLoad, RepoSection, TabCounts,
};

use crate::{
    engine::{RostrumCore, state::parse_repo},
    error::RostrumError,
};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The feed from memory, filled from the SQLite cache on first call. No
    /// network; paints the first frame.
    pub async fn cached_feed(&self) -> Result<FeedSnapshot, RostrumError> {
        self.ensure_hydrated().await?;
        self.actor.call(|state| state.snapshot()).await
    }

    /// Fetch every watched repository (a few at a time), their distance from
    /// base, and cache the result. A repository that fails keeps its previous
    /// pull requests and reports the failure in its section; the call itself
    /// fails only for `NotSignedIn` and `GitHubAuthFailed`. Merge states
    /// GitHub is still computing are re-checked in the background and
    /// delivered through the observer.
    pub async fn refresh_feed(&self) -> Result<FeedSnapshot, RostrumError> {
        self.refresh(Scope::All, Probes::Schedule).await
    }

    /// Fetch one repository, e.g. just after adding it.
    pub async fn refresh_repo(&self, repo: String) -> Result<FeedSnapshot, RostrumError> {
        let id = parse_repo(&repo)?;
        self.refresh(Scope::One(id), Probes::Schedule).await
    }

    /// Set the search box: matches title, number, author and label names.
    /// Not persisted.
    pub async fn set_query(&self, query: String) -> Result<FeedSnapshot, RostrumError> {
        self.actor
            .call(move |state| {
                state.feed.filter.query = query.trim().to_string();
                state.publish()
            })
            .await
    }

    /// Replace the persisted filter preferences.
    pub async fn set_filter(
        &self,
        preferences: FeedPreferences,
    ) -> Result<FeedSnapshot, RostrumError> {
        self.actor
            .try_call(move |state| {
                let mut filter = state.feed.filter.clone();
                filter.hide_drafts = preferences.hide_drafts;
                filter.hide_empty_repos = preferences.hide_empty_repos;
                filter.include_involved = preferences.include_involved;
                filter.authors = preferences
                    .authors
                    .iter()
                    .map(|login| LoginKey::new(login))
                    .filter(|login| !login.is_empty())
                    .collect();
                state.edit_config(|config| config.absorb_filter(&filter))?;
                state.feed.filter = filter;
                Ok(state.publish())
            })
            .await
    }

    /// Add or remove one author from the filter (case-insensitive).
    pub async fn toggle_author(&self, login: String) -> Result<FeedSnapshot, RostrumError> {
        let key = LoginKey::new(&login);
        if key.is_empty() {
            return Err(RostrumError::invalid("a login cannot be blank"));
        }
        self.actor
            .try_call(move |state| {
                let mut filter = state.feed.filter.clone();
                filter.toggle_author(key);
                state.edit_config(|config| config.absorb_filter(&filter))?;
                state.feed.filter = filter;
                Ok(state.publish())
            })
            .await
    }

    /// Reset the query and every filter preference to their defaults. The
    /// sort is not a filter and is kept, as on the desktop.
    pub async fn clear_filter(&self) -> Result<FeedSnapshot, RostrumError> {
        self.actor
            .try_call(|state| {
                let filter = state.feed.filter.cleared();
                state.edit_config(|config| config.absorb_filter(&filter))?;
                state.feed.filter = filter;
                Ok(state.publish())
            })
            .await
    }

    /// Collapse or expand a repository's container. Not persisted.
    pub async fn toggle_collapsed(&self, repo: String) -> Result<FeedSnapshot, RostrumError> {
        let id = parse_repo(&repo)?;
        self.actor
            .try_call(move |state| {
                if !state.feed.toggle_collapsed(&id) {
                    return Err(RostrumError::invalid(format!("{id} is not watched")));
                }
                Ok(state.publish())
            })
            .await
    }

    /// Show the Pull requests or the Issues list. Persisted.
    pub async fn set_feed_tab(&self, tab: FeedTab) -> Result<FeedSnapshot, RostrumError> {
        self.actor
            .try_call(move |state| {
                let tab = rostrum_core::FeedTab::from(tab);
                state.edit_config(|config| config.feed_tab = tab)?;
                state.feed.tab = tab;
                Ok(state.publish())
            })
            .await
    }

    /// The people the author filter can be pointed at, for the active tab:
    /// pull request authors, or issue authors. `limit` caps the unselected
    /// tail; `None` returns everyone.
    pub async fn author_roster(&self, limit: Option<u32>) -> Result<AuthorRoster, RostrumError> {
        self.actor
            .call(move |state| {
                let selected = state.feed.filter.authors.clone();
                let viewer = state.session.viewer();
                let entries = match state.feed.tab {
                    rostrum_core::FeedTab::PullRequests => {
                        roster(&state.feed.repos, viewer, &selected)
                    }
                    rostrum_core::FeedTab::Issues => {
                        issue_roster(&state.feed.repos, viewer, &selected)
                    }
                };
                let cap = limit.map_or(usize::MAX, |limit| limit as usize);
                let shown = visible(entries, &selected, cap);
                AuthorRoster {
                    authors: shown
                        .shown
                        .into_iter()
                        .map(|entry| AuthorChip {
                            selected: selected.contains(&entry.key),
                            login: entry.user.login,
                            avatar_url: entry.user.avatar_url,
                            open_items: count(entry.open_prs),
                            is_viewer: entry.is_viewer,
                        })
                        .collect(),
                    hidden: count(shown.hidden),
                }
            })
            .await
    }

    /// Register (or with `None`, remove) the observer that receives every
    /// feed change. A newly registered observer receives the current feed at
    /// once.
    pub async fn set_feed_observer(
        &self,
        observer: Option<Arc<dyn FeedObserver>>,
    ) -> Result<(), RostrumError> {
        self.actor
            .call(move |state| {
                state.observed = observer.is_some();
                state.notifier.set_observer(observer);
                if state.observed {
                    state.notifier.publish(state.snapshot());
                }
            })
            .await
    }
}
