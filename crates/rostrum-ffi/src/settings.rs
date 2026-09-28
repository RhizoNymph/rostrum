//! Persisted settings: the repository list, refresh cadence, notification
//! toggles, and the feed's standing preferences.
//!
//! Stored in `<data_dir>/config.json` through `rostrum-config`, the desktop's
//! own schema, so the two files are interchangeable. No secret is ever
//! written there.

use crate::{engine::RostrumCore, error::RostrumError, feed::FeedPreferences};

/// Everything on the settings screen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Settings {
    /// Watched repositories, `owner/name`, sorted.
    pub repos: Vec<String>,
    /// Seconds between foreground feed refreshes (clamped to 10..=3600).
    pub refresh_interval_secs: u64,
    /// Open pull requests fetched per repository (1..=100).
    pub prs_per_repo: u32,
    /// Notify when a pull request appears in a watched repository.
    pub notify_new_pull_requests: bool,
    /// Notify when your review is newly requested.
    pub notify_review_requests: bool,
    /// Default for the "stash local changes" checkbox on desktop jobs.
    pub autostash: bool,
    /// The feed's persisted filter preferences.
    pub feed: FeedPreferences,
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    pub async fn settings(&self) -> Result<Settings, RostrumError> {
        Err(RostrumError::unimplemented("settings"))
    }

    /// Add a repository from `owner/name` or a pasted GitHub URL, returning
    /// its normalised `owner/name`. Fails with `InvalidRepo` for malformed
    /// input and `DuplicateRepo` when it is already watched. The repository
    /// starts out `Idle`; call `refresh_repo` to fetch it.
    pub async fn add_repo(&self, input: String) -> Result<String, RostrumError> {
        let _ = input;
        Err(RostrumError::unimplemented("add_repo"))
    }

    /// Stop watching a repository. Returns whether it was watched. Pending
    /// review drafts on its pull requests are kept.
    pub async fn remove_repo(&self, repo: String) -> Result<bool, RostrumError> {
        let _ = repo;
        Err(RostrumError::unimplemented("remove_repo"))
    }

    /// Set the foreground refresh interval; out-of-range values are clamped.
    pub async fn set_refresh_interval(&self, seconds: u64) -> Result<Settings, RostrumError> {
        let _ = seconds;
        Err(RostrumError::unimplemented("set_refresh_interval"))
    }

    /// Set how many open pull requests are fetched per repository; clamped
    /// to 1..=100.
    pub async fn set_prs_per_repo(&self, count: u32) -> Result<Settings, RostrumError> {
        let _ = count;
        Err(RostrumError::unimplemented("set_prs_per_repo"))
    }

    pub async fn set_notifications(
        &self,
        new_pull_requests: bool,
        review_requests: bool,
    ) -> Result<Settings, RostrumError> {
        let _ = (new_pull_requests, review_requests);
        Err(RostrumError::unimplemented("set_notifications"))
    }

    pub async fn set_autostash(&self, autostash: bool) -> Result<Settings, RostrumError> {
        let _ = autostash;
        Err(RostrumError::unimplemented("set_autostash"))
    }
}
