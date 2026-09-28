//! The feed: every watched repository's open pull requests, filtered.

mod types;

use std::sync::Arc;

pub use types::{
    AuthorChip, AuthorRoster, BaseDivergence, FeedObserver, FeedPreferences, FeedSnapshot,
    PrSummary, RepoBody, RepoLoad, RepoSection,
};

use crate::{engine::RostrumCore, error::RostrumError};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The feed from memory, filled from the SQLite cache on first call. No
    /// network; paints the first frame.
    pub async fn cached_feed(&self) -> Result<FeedSnapshot, RostrumError> {
        Err(RostrumError::unimplemented("cached_feed"))
    }

    /// Fetch every watched repository (a few at a time), their distance from
    /// base, and cache the result. A repository that fails keeps its previous
    /// pull requests and reports the failure in its section; the call itself
    /// fails only for `NotSignedIn` and `GitHubAuthFailed`. Merge states
    /// GitHub is still computing are re-checked in the background and
    /// delivered through the observer.
    pub async fn refresh_feed(&self) -> Result<FeedSnapshot, RostrumError> {
        Err(RostrumError::unimplemented("refresh_feed"))
    }

    /// Fetch one repository, e.g. just after adding it.
    pub async fn refresh_repo(&self, repo: String) -> Result<FeedSnapshot, RostrumError> {
        let _ = repo;
        Err(RostrumError::unimplemented("refresh_repo"))
    }

    /// Set the search box: matches title, number, author and label names.
    /// Not persisted.
    pub async fn set_query(&self, query: String) -> Result<FeedSnapshot, RostrumError> {
        let _ = query;
        Err(RostrumError::unimplemented("set_query"))
    }

    /// Replace the persisted filter preferences.
    pub async fn set_filter(
        &self,
        preferences: FeedPreferences,
    ) -> Result<FeedSnapshot, RostrumError> {
        let _ = preferences;
        Err(RostrumError::unimplemented("set_filter"))
    }

    /// Add or remove one author from the filter (case-insensitive).
    pub async fn toggle_author(&self, login: String) -> Result<FeedSnapshot, RostrumError> {
        let _ = login;
        Err(RostrumError::unimplemented("toggle_author"))
    }

    /// Reset the query and every preference to their defaults.
    pub async fn clear_filter(&self) -> Result<FeedSnapshot, RostrumError> {
        Err(RostrumError::unimplemented("clear_filter"))
    }

    /// Collapse or expand a repository's container. Not persisted.
    pub async fn toggle_collapsed(&self, repo: String) -> Result<FeedSnapshot, RostrumError> {
        let _ = repo;
        Err(RostrumError::unimplemented("toggle_collapsed"))
    }

    /// The people the author filter can be pointed at. `limit` caps the
    /// unselected tail; `None` returns everyone.
    pub async fn author_roster(&self, limit: Option<u32>) -> Result<AuthorRoster, RostrumError> {
        let _ = limit;
        Err(RostrumError::unimplemented("author_roster"))
    }

    /// Register (or with `None`, remove) the observer that receives every
    /// feed change.
    pub async fn set_feed_observer(
        &self,
        observer: Option<Arc<dyn FeedObserver>>,
    ) -> Result<(), RostrumError> {
        let _ = observer;
        Err(RostrumError::unimplemented("set_feed_observer"))
    }
}
