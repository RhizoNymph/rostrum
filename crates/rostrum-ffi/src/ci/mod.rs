//! The CI grid on the phone: every open pull request's checks in the feed's
//! order, job logs, another app's check output, and re-runs.
//!
//! The rules are `rostrum_core::ci`'s, shared with the desktop's CI view
//! (`docs/features/ci_grid.md`): the matrix (`build_grid`), timing against an
//! explicit `now` (`Timing`), log parsing (`parse_log`), re-run eligibility
//! (`rerun_targets`) and the optimistic flip (`mark_requeued`). The requests
//! are `rostrum-github`'s (`ci_checks`, `job_log`, `check_output`, `rerun`).
//!
//! Checks are fetched only when asked (`refresh_ci`, `refresh_ci_repo`),
//! never with the feed or the background notification check. `ci_grid`
//! rebuilds from what is held, so Kotlin calls it once a second while the
//! grid says `ticks`, and `refresh_ci` every 15 s while it says
//! `any_running`, as the desktop does.

mod convert;
mod types;

use std::{sync::Arc, time::Duration};

use chrono::Utc;
use futures::{StreamExt, stream};
use rostrum_core::{
    PrNumber, RepoId,
    ci::{
        CheckKey, CiChecks, DEFAULT_LOG_LINES, GridFilter, LineLimit, build_grid, mark_requeued,
        parse_log, rerun_targets,
    },
};
use rostrum_github::{GitHubClient, GitHubError, ci::RerunError};

pub use types::*;

use crate::{
    engine::{
        RostrumCore,
        actor::{Actor, WeakActor},
        state::{CoreState, parse_repo},
    },
    error::RostrumError,
};

/// Repositories fetched at once.
const CONCURRENT_FETCHES: usize = 4;

/// After a re-run is accepted, how long before re-fetching: long enough for
/// GitHub to have created the new attempt.
const AFTER_RERUN: Duration = Duration::from_secs(4);

/// Recent job logs kept, raw, so "load full" re-parses without a request.
pub(crate) const RECENT_LOGS: usize = 4;

impl From<CiGridFilter> for GridFilter {
    fn from(filter: CiGridFilter) -> Self {
        Self {
            needs_attention: filter.needs_attention,
        }
    }
}

fn grid_of(state: &CoreState, filter: CiGridFilter) -> CiGrid {
    let grid = build_grid(
        &state.feed.repos,
        &state.feed.filter,
        &state.ci,
        filter.into(),
    );
    convert::grid(&grid, state.ci.any_running(), Utc::now())
}

/// Fetch `repos`' checks and store them, at most four at a time. A failure
/// stays in its repository; a rejected token is returned for the caller to
/// report.
async fn fetch_into(
    actor: &Actor,
    client: &GitHubClient,
    repos: Vec<RepoId>,
    limit: u32,
) -> Result<Option<GitHubError>, RostrumError> {
    let starting = repos.clone();
    actor
        .call(move |state| {
            for repo in &starting {
                state.ci.begin(repo);
            }
        })
        .await?;
    let mut landing = stream::iter(repos)
        .map(|repo| async move {
            let outcome = client.ci_checks(&repo, limit).await;
            (repo, outcome)
        })
        .buffer_unordered(CONCURRENT_FETCHES);
    let mut rejected = None;
    while let Some((repo, outcome)) = landing.next().await {
        let outcome = match outcome {
            Ok(fetched) => Ok(fetched.checks),
            Err(error) => {
                tracing::warn!(%repo, %error, "checks fetch failed");
                let message = error.to_string();
                if matches!(error, GitHubError::Unauthorized) {
                    rejected = Some(error);
                }
                Err(message)
            }
        };
        actor
            .call(move |state| match outcome {
                Ok(checks) => {
                    tracing::debug!(%repo, prs = checks.len(), "checks fetched");
                    state.ci.loaded(&repo, checks, Utc::now());
                }
                Err(message) => state.ci.failed(&repo, message, Utc::now()),
            })
            .await?;
    }
    Ok(rejected)
}

/// Re-fetch one repository's checks after `delay`, for as long as the core
/// lives.
fn refetch_later(me: WeakActor, client: GitHubClient, repo: RepoId, limit: u32, delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let Some(actor) = me.upgrade() else {
            return;
        };
        if let Err(error) = fetch_into(&actor, &client, vec![repo.clone()], limit).await {
            tracing::debug!(%repo, %error, "re-fetch after a re-run did not land");
        }
    });
}

impl RostrumCore {
    async fn fetch_ci(&self, only: Option<RepoId>) -> Result<(), RostrumError> {
        self.ensure_hydrated().await?;
        let (client, repos, limit) = self
            .actor
            .try_call(move |state| {
                let repos = match only {
                    Some(repo) => {
                        if !state.feed.repos.iter().any(|known| known.id == repo) {
                            return Err(RostrumError::invalid(format!(
                                "{repo} is not in the repository list"
                            )));
                        }
                        vec![repo]
                    }
                    None => state.feed.repo_ids(),
                };
                Ok((
                    state.github()?,
                    repos,
                    state.config.prs_per_repo.clamp(1, 100),
                ))
            })
            .await?;
        if let Some(rejected) = fetch_into(&self.actor, &client, repos, limit).await? {
            return Err(self.github_failed(rejected).await);
        }
        Ok(())
    }

    async fn ci_client(&self, repo: &str) -> Result<(RepoId, GitHubClient), RostrumError> {
        let id = parse_repo(repo)?;
        let client = self.actor.try_call(|state| state.github()).await?;
        Ok((id, client))
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The grid from the checks held, as of now. No network: call it once a
    /// second while `ticks`, and after `refresh_ci`.
    pub async fn ci_grid(&self, filter: CiGridFilter) -> Result<CiGrid, RostrumError> {
        self.ensure_hydrated().await?;
        self.actor.call(move |state| grid_of(state, filter)).await
    }

    /// Fetch every watched repository's checks (one request each), then the
    /// grid. A repository that fails keeps what it had and says so in its
    /// section.
    pub async fn refresh_ci(&self, filter: CiGridFilter) -> Result<CiGrid, RostrumError> {
        self.fetch_ci(None).await?;
        self.actor.call(move |state| grid_of(state, filter)).await
    }

    /// [`Self::refresh_ci`] for one watched repository.
    pub async fn refresh_ci_repo(
        &self,
        repo: String,
        filter: CiGridFilter,
    ) -> Result<CiGrid, RostrumError> {
        self.fetch_ci(Some(parse_repo(&repo)?)).await?;
        self.actor.call(move |state| grid_of(state, filter)).await
    }

    /// An Actions job's log, parsed: lines by kind, collapsible groups, steps,
    /// the first error and its step. A long log keeps its last 20 000 lines
    /// (`truncated`) unless `full`; the raw text is kept, so asking for the
    /// full log right after re-parses it without a request.
    pub async fn job_log(
        &self,
        repo: String,
        job_id: u64,
        full: bool,
    ) -> Result<CiJobLog, RostrumError> {
        let (id, client) = self.ci_client(&repo).await?;
        let key = (id.clone(), job_id);
        let lookup = key.clone();
        let held = self
            .actor
            .call(move |state| state.job_logs.get(&lookup).cloned())
            .await?;
        let raw = match held {
            Some(raw) => raw,
            None => {
                let raw = Arc::new(self.github(client.job_log(&id, job_id).await).await?);
                let kept = raw.clone();
                self.actor
                    .call(move |state| state.job_logs.insert(key, kept))
                    .await?;
                raw
            }
        };
        let limit = if full {
            LineLimit::Full
        } else {
            LineLimit::Last(DEFAULT_LOG_LINES)
        };
        Ok(tokio::task::spawn_blocking(move || convert::job_log(&parse_log(&raw, limit))).await?)
    }

    /// Another app's check run: its output (markdown flattened) and
    /// annotations. Two requests.
    pub async fn check_output(
        &self,
        repo: String,
        check_run_id: u64,
    ) -> Result<CiCheckOutput, RostrumError> {
        let (id, client) = self.ci_client(&repo).await?;
        let output = self
            .github(client.check_output(&id, check_run_id).await)
            .await?;
        Ok(tokio::task::spawn_blocking(move || convert::check_output(&output, &id)).await?)
    }

    /// The re-runs the check in column `key` of pull request `pr` offers,
    /// from the checks held. `InvalidInput` when that cell is "not run".
    pub async fn rerun_targets(
        &self,
        repo: String,
        pr: u32,
        key: CiCheckKey,
    ) -> Result<CiRerunChoice, RostrumError> {
        let id = parse_repo(&repo)?;
        let key = CheckKey::from(key);
        self.actor
            .try_call(move |state| {
                let checks = state.ci.pr(&id, PrNumber(pr)).ok_or_else(|| {
                    RostrumError::invalid(format!("no checks fetched for {id}#{pr}"))
                })?;
                let latest = checks.latest();
                let entry = latest.get(&key).ok_or_else(|| {
                    RostrumError::invalid(format!("“{key}” has not run on {id}#{pr}"))
                })?;
                Ok(convert::rerun_choice(rerun_targets(checks, entry), entry))
            })
            .await
    }

    /// Ask GitHub to re-run `target`. The checks it covers show as queued at
    /// once; on success the repository is re-fetched a few seconds later,
    /// once GitHub has the new attempt, and on a refusal at once, which puts
    /// the old result back. Refusals are typed: `CiNoPermission`,
    /// `CiNotRerunnable`, `CiNotFound`.
    pub async fn rerun(&self, repo: String, target: CiRerun) -> Result<(), RostrumError> {
        let (id, client) = self.ci_client(&repo).await?;
        let target = rostrum_core::ci::RerunTarget::from(target);
        let flip = id.clone();
        let (me, limit) = self
            .actor
            .call(move |state| {
                let marked = requeue(&mut state.ci, &flip, target);
                tracing::debug!(repo = %flip, ?target, marked, "checks marked queued");
                (state.me.clone(), state.config.prs_per_repo.clamp(1, 100))
            })
            .await?;
        match client.rerun(&id, target).await {
            Ok(()) => {
                refetch_later(me, client, id, limit, AFTER_RERUN);
                Ok(())
            }
            Err(error) => {
                tracing::info!(repo = %id, ?target, %error, "re-run refused");
                if let Err(refetch) = fetch_into(&self.actor, &client, vec![id], limit).await {
                    tracing::debug!(%refetch, "re-fetch after a refused re-run failed");
                }
                Err(match error {
                    RerunError::NoPermission { message } => {
                        RostrumError::CiNoPermission { reason: message }
                    }
                    RerunError::NotRerunnable { message } => {
                        RostrumError::CiNotRerunnable { reason: message }
                    }
                    RerunError::NotFound => RostrumError::CiNotFound,
                    RerunError::Api(error) => self.github_failed(error).await,
                })
            }
        }
    }
}

/// Flip every check `target` covers in `repo` to queued.
fn requeue(ci: &mut CiChecks, repo: &RepoId, target: rostrum_core::ci::RerunTarget) -> usize {
    let numbers: Vec<PrNumber> = ci
        .repo(repo)
        .map(|held| held.prs.keys().copied().collect())
        .unwrap_or_default();
    let mut marked = 0;
    for number in numbers {
        if let Some(checks) = ci.pr_mut(repo, number) {
            marked += mark_requeued(checks, target);
        }
    }
    marked
}
