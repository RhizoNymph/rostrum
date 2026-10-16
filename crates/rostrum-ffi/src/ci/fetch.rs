//! Which repositories' checks the grid fetches, and fetching them in
//! batches.
//!
//! The decisions are `rostrum_core::poll`'s, the desktop's own: opening the
//! grid fetches only repositories where something could have changed
//! (`plan_ci` with the timer's trigger), the watch every 15 s only those with
//! anything queued or running (`running_repos`), and pull to refresh every
//! repository whose pull requests have loaded (`plan_ci`, explicit). Nothing
//! is sent while GitHub has paused polling.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use futures::StreamExt;
use rostrum_core::{
    RepoId,
    budget::{Feature, Pause},
    ci::PrChecks,
    poll::{CI_SAFETY_INTERVAL, Trigger, plan_ci, running_repos},
};
use rostrum_github::{GitHubClient, GitHubError, RepoBatch};

use crate::{
    budget::is_rate_limit,
    engine::{actor::WeakActor, state::CoreState},
    error::RostrumError,
    feed::check_batches,
};

/// Why the grid fetches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CiFetch {
    /// The grid opened: what could have changed since the last fetch.
    Opened,
    /// The 15 s watch: what is queued or running.
    Watch,
    /// The user asked: everything.
    Explicit,
}

/// What [`CoreState::plan_checks`] decided.
pub(crate) enum CiPlan {
    /// GitHub has paused polling: nothing is sent.
    Paused(Pause),
    Fetch {
        client: GitHubClient,
        repos: Vec<RepoId>,
        limit: u32,
    },
}

impl CoreState {
    /// The repositories `why` fetches, among `only` (every watched one when
    /// `None`), marked as loading.
    pub(crate) fn plan_checks(
        &mut self,
        why: CiFetch,
        only: Option<&BTreeSet<RepoId>>,
        now: DateTime<Utc>,
    ) -> Result<CiPlan, RostrumError> {
        let client = self.github_for(Feature::Ci)?;
        if let Some(pause) = self.active_pause(now) {
            tracing::debug!(?why, until = %pause.until(), "checks skipped: rate limit pause");
            return Ok(CiPlan::Paused(pause));
        }
        let repos: Vec<RepoId> = match why {
            CiFetch::Opened | CiFetch::Explicit => {
                let trigger = if why == CiFetch::Explicit {
                    Trigger::Explicit
                } else {
                    Trigger::Timer
                };
                let planned = plan_ci(
                    &self.feed.repos,
                    &self.ci,
                    now,
                    CI_SAFETY_INTERVAL,
                    trigger,
                    only,
                );
                for (repo, reason) in &planned {
                    tracing::debug!(%repo, reason = reason.label(), "checks due");
                }
                planned.into_iter().map(|(repo, _)| repo).collect()
            }
            CiFetch::Watch => running_repos(&self.feed.repos, &self.ci)
                .into_iter()
                .filter(|repo| only.is_none_or(|only| only.contains(repo)))
                .collect(),
        };
        for repo in &repos {
            self.ci.begin(repo);
        }
        Ok(CiPlan::Fetch {
            client,
            repos,
            limit: self.config.prs_per_repo.clamp(1, 100),
        })
    }

    /// Apply one chunk of checks. Returns whether GitHub rejected the token.
    /// A rate-limit refusal keeps the checks held without marking them
    /// failed.
    pub(crate) fn apply_checks_batch(
        &mut self,
        chunk: &[RepoId],
        outcome: Result<RepoBatch<Vec<PrChecks>>, GitHubError>,
    ) -> bool {
        let now = Utc::now();
        match outcome {
            Ok(batch) => {
                for (repo, result) in batch.results {
                    match result {
                        Ok(checks) => {
                            tracing::debug!(%repo, prs = checks.len(), "checks fetched");
                            self.ci.loaded(&repo, checks, now);
                        }
                        Err(error) => self.checks_failed(&repo, &error, now),
                    }
                }
                false
            }
            Err(error) => {
                for repo in chunk {
                    self.checks_failed(repo, &error, now);
                }
                self.note_github_error(&error);
                matches!(error, GitHubError::Unauthorized)
            }
        }
    }

    fn checks_failed(&mut self, repo: &RepoId, error: &GitHubError, now: DateTime<Utc>) {
        tracing::warn!(%repo, %error, "checks fetch failed");
        let held = self
            .ci
            .repo(repo)
            .is_some_and(|checks| !checks.prs.is_empty());
        if is_rate_limit(error) && held {
            return;
        }
        self.ci.failed(repo, error.to_string(), now);
    }
}

/// Fetch `repos`' checks in batches and store each as it lands. Fails with
/// `GitHubAuthFailed` when GitHub rejected the token; any other failure stays
/// in its repository.
pub(crate) async fn land_checks(
    me: &WeakActor,
    client: &GitHubClient,
    repos: Vec<RepoId>,
    limit: u32,
) -> Result<(), RostrumError> {
    let mut landing = check_batches(client, repos, limit);
    let mut rejected = false;
    while let Some((chunk, outcome)) = landing.next().await {
        rejected |= me
            .call(move |state| state.apply_checks_batch(&chunk, outcome))
            .await?;
    }
    if rejected {
        return Err(GitHubError::Unauthorized.into());
    }
    Ok(())
}
