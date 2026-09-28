//! Persisted settings: the repository list, refresh cadence, notification
//! toggles, and the feed's standing preferences.
//!
//! Stored in `<data_dir>/config.json` through `rostrum-config`, the desktop's
//! own schema, so the two files are interchangeable. No secret is ever
//! written there.

use rostrum_config::{AddRepoError, Config};

use crate::{
    engine::{
        RostrumCore,
        state::{CoreState, parse_repo},
    },
    error::RostrumError,
    feed::FeedPreferences,
};

/// Bounds on the foreground refresh interval, matching the desktop's clamp:
/// a pathological value must not become a request storm.
const REFRESH_SECS: std::ops::RangeInclusive<u64> = 10..=3600;
/// GitHub's page size caps a single query at 100.
const PRS_PER_REPO: std::ops::RangeInclusive<u32> = 1..=100;

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

impl CoreState {
    pub(crate) fn settings(&self) -> Settings {
        settings_of(&self.config, self.feed.preferences())
    }
}

fn settings_of(config: &Config, feed: FeedPreferences) -> Settings {
    Settings {
        repos: config.repos.clone(),
        refresh_interval_secs: config
            .refresh_secs
            .clamp(*REFRESH_SECS.start(), *REFRESH_SECS.end()),
        prs_per_repo: config
            .prs_per_repo
            .clamp(*PRS_PER_REPO.start(), *PRS_PER_REPO.end()),
        notify_new_pull_requests: config.notifications,
        notify_review_requests: config.notify_review_requests,
        autostash: config.autostash,
        feed,
    }
}

/// `add_repo`'s failure, typed for the UI.
fn add_repo_error(input: &str, error: AddRepoError) -> RostrumError {
    match error {
        AddRepoError::Malformed(reason) => RostrumError::InvalidRepo {
            input: input.trim().to_string(),
            reason: reason.to_string(),
        },
        AddRepoError::Duplicate(repo) => RostrumError::DuplicateRepo {
            repo: repo.to_string(),
        },
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    pub async fn settings(&self) -> Result<Settings, RostrumError> {
        self.actor.call(|state| state.settings()).await
    }

    /// Add a repository from `owner/name` or a pasted GitHub URL, returning
    /// its normalised `owner/name`. Fails with `InvalidRepo` for malformed
    /// input and `DuplicateRepo` when it is already watched. The repository
    /// starts out `Idle`; call `refresh_repo` to fetch it.
    pub async fn add_repo(&self, input: String) -> Result<String, RostrumError> {
        self.actor
            .try_call(move |state| {
                let mut added = None;
                let mut failure = None;
                state.edit_config(|config| match config.try_add_repo(&input) {
                    Ok(id) => added = Some(id),
                    Err(error) => failure = Some(error),
                })?;
                if let Some(error) = failure {
                    return Err(add_repo_error(&input, error));
                }
                let id =
                    added.ok_or_else(|| RostrumError::internal("add_repo reported nothing"))?;
                let order = state.config.repos.clone();
                state.feed.add_repo(id.clone(), &order);
                state.publish();
                tracing::info!(repo = %id, "repository added");
                Ok(id.to_string())
            })
            .await
    }

    /// Stop watching a repository. Returns whether it was watched. Pending
    /// review drafts on its pull requests are kept.
    pub async fn remove_repo(&self, repo: String) -> Result<bool, RostrumError> {
        let id = parse_repo(&repo)?;
        self.actor
            .try_call(move |state| {
                let mut removed = false;
                state.edit_config(|config| removed = config.remove_repo(&id))?;
                if removed {
                    state.feed.remove_repo(&id);
                    state.probes.remove(&id);
                    state.labels.remove(&id);
                    state.conversations.retain(|key| key.repo != id);
                    state.files.retain(|key| key.repo != id);
                    state.publish();
                    tracing::info!(repo = %id, "repository removed");
                }
                Ok(removed)
            })
            .await
    }

    /// Set the foreground refresh interval; out-of-range values are clamped.
    pub async fn set_refresh_interval(&self, seconds: u64) -> Result<Settings, RostrumError> {
        let seconds = seconds.clamp(*REFRESH_SECS.start(), *REFRESH_SECS.end());
        self.edit_settings(move |config| config.refresh_secs = seconds)
            .await
    }

    /// Set how many open pull requests are fetched per repository; clamped
    /// to 1..=100.
    pub async fn set_prs_per_repo(&self, count: u32) -> Result<Settings, RostrumError> {
        let count = count.clamp(*PRS_PER_REPO.start(), *PRS_PER_REPO.end());
        self.edit_settings(move |config| config.prs_per_repo = count)
            .await
    }

    pub async fn set_notifications(
        &self,
        new_pull_requests: bool,
        review_requests: bool,
    ) -> Result<Settings, RostrumError> {
        self.edit_settings(move |config| {
            config.notifications = new_pull_requests;
            config.notify_review_requests = review_requests;
        })
        .await
    }

    pub async fn set_autostash(&self, autostash: bool) -> Result<Settings, RostrumError> {
        self.edit_settings(move |config| config.autostash = autostash)
            .await
    }
}

impl RostrumCore {
    async fn edit_settings(
        &self,
        edit: impl FnOnce(&mut Config) + Send + 'static,
    ) -> Result<Settings, RostrumError> {
        self.actor
            .try_call(move |state| {
                state.edit_config(edit)?;
                Ok(state.settings())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_clamp_what_a_hand_edited_file_may_hold() {
        let config = Config {
            refresh_secs: 1,
            prs_per_repo: 500,
            ..Default::default()
        };
        let settings = settings_of(
            &config,
            FeedPreferences {
                hide_drafts: false,
                hide_empty_repos: true,
                authors: vec![],
                include_involved: false,
            },
        );
        assert_eq!(settings.refresh_interval_secs, 10);
        assert_eq!(settings.prs_per_repo, 100);
    }

    #[test]
    fn add_repo_errors_are_typed() {
        let mut config = Config {
            repos: vec!["a/b".into()],
            ..Default::default()
        };
        let duplicate = config.try_add_repo("a/b").expect_err("duplicate");
        assert_eq!(
            add_repo_error("a/b", duplicate),
            RostrumError::DuplicateRepo { repo: "a/b".into() }
        );
        let malformed = config.try_add_repo(" nope ").expect_err("malformed");
        assert!(matches!(
            add_repo_error(" nope ", malformed),
            RostrumError::InvalidRepo { input, .. } if input == "nope"
        ));
    }
}
