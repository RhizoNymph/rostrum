//! The CI grid on the phone: one matrix of every open, non-draft pull
//! request's checks across the watched repositories in the feed's order, job
//! logs, another app's check output, and re-runs.
//!
//! The rules are `rostrum_core::ci`'s, shared with the desktop's CI view
//! (`docs/features/ci_grid.md`): the matrix (`build_grid`), timing against an
//! explicit `now` (`Timing`), log parsing (`parse_log`), re-run eligibility
//! (`rerun_targets`) and the optimistic flip (`mark_requeued`). The requests
//! are `rostrum-github`'s (`ci_checks`, `job_log`, `check_output`, `rerun`).
//!
//! Checks are fetched only when asked, never with the feed or the
//! background notification check, and only where the desktop's skip rules
//! say they could have changed (`fetch`): `open_ci` when the grid opens,
//! `watch_ci` every 15 s while the grid says `any_running` (only what is
//! queued or running), `refresh_ci` on pull to refresh (everything). Each
//! sends as few batched documents as the cost model allows, and nothing
//! while GitHub has paused polling. `ci_grid` rebuilds from what is held, so
//! Kotlin calls it once a second while the grid says `ticks`.

mod convert;
mod fetch;
mod types;

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use chrono::Utc;
use fetch::{CiFetch, CiPlan, land_checks};
use rostrum_core::{
    PrNumber, RepoId,
    ci::{
        CheckKey, CiChecks, DEFAULT_LOG_LINES, GridFilter, LineLimit, build_grid, mark_requeued,
        parse_log, rerun_targets,
    },
};
use rostrum_github::{GitHubClient, ci::RerunError};

pub use types::*;

use crate::{
    budget::paused_error,
    engine::{
        RostrumCore,
        actor::WeakActor,
        state::{CoreState, parse_repo},
    },
    error::RostrumError,
};

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

/// Fetch the checks `why` calls for among `only`, for as long as the core
/// lives. `Ok(Some(pause))` when GitHub has paused polling and nothing was
/// sent.
async fn fetch_checks(
    me: &WeakActor,
    why: CiFetch,
    only: Option<BTreeSet<RepoId>>,
) -> Result<Option<rostrum_core::budget::Pause>, RostrumError> {
    let plan = me
        .call(move |state| state.plan_checks(why, only.as_ref(), Utc::now()))
        .await??;
    let (client, repos, limit) = match plan {
        CiPlan::Paused(pause) => return Ok(Some(pause)),
        CiPlan::Fetch {
            client,
            repos,
            limit,
        } => (client, repos, limit),
    };
    let fetched = land_checks(me, &client, repos, limit).await;
    let _ = me
        .call(|state| state.log_spend_if_due(std::time::Instant::now()))
        .await;
    fetched.map(|()| None)
}

/// Re-fetch one repository's checks after `delay`, for as long as the core
/// lives and GitHub has not paused polling.
fn refetch_later(me: WeakActor, repo: RepoId, delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let only = BTreeSet::from([repo.clone()]);
        if let Err(error) = fetch_checks(&me, CiFetch::Explicit, Some(only)).await {
            tracing::debug!(%repo, %error, "re-fetch after a re-run did not land");
        }
    });
}

impl RostrumCore {
    /// Fetch what `why` calls for: every watched repository, or just
    /// `repo`. Explicit requests fail with `GitHubRateLimited` while GitHub
    /// has paused polling; the grid's own fetches skip quietly.
    async fn fetch_ci(&self, why: CiFetch, repo: Option<RepoId>) -> Result<(), RostrumError> {
        self.ensure_hydrated().await?;
        if let Some(repo) = &repo {
            let known = repo.clone();
            let watched = self
                .actor
                .call(move |state| state.feed.repo(&known).is_some())
                .await?;
            if !watched {
                return Err(RostrumError::invalid(format!(
                    "{repo} is not in the repository list"
                )));
            }
        }
        let only = repo.map(|repo| BTreeSet::from([repo]));
        // A rejected token is already noted in the session as the batch
        // lands.
        let paused = fetch_checks(&self.actor.downgrade(), why, only).await?;
        match paused {
            Some(pause) if why == CiFetch::Explicit => Err(paused_error(&pause)),
            _ => Ok(()),
        }
    }

    async fn ci_client(&self, repo: &str) -> Result<(RepoId, GitHubClient), RostrumError> {
        let id = parse_repo(repo)?;
        let client = self.actor.try_call(|state| state.github()).await?;
        Ok((id, client))
    }

    async fn grid_now(&self, filter: CiGridFilter) -> Result<CiGrid, RostrumError> {
        self.actor.call(move |state| grid_of(state, filter)).await
    }
}

/// `owner/name` Kotlin passed, if any.
fn repo_arg(repo: Option<String>) -> Result<Option<RepoId>, RostrumError> {
    repo.as_deref().map(parse_repo).transpose()
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The grid from the checks held, as of now. No network: call it once a
    /// second while `ticks`, and after `refresh_ci`.
    pub async fn ci_grid(&self, filter: CiGridFilter) -> Result<CiGrid, RostrumError> {
        self.ensure_hydrated().await?;
        self.actor.call(move |state| grid_of(state, filter)).await
    }

    /// Fetch every watched repository's checks (pull to refresh), in as few
    /// batched requests as the cost model allows, then the grid. A
    /// repository that fails keeps what it had and says so in its
    /// `CiGrid::repos` entry. `GitHubRateLimited` while GitHub has paused
    /// polling (nothing is sent).
    pub async fn refresh_ci(&self, filter: CiGridFilter) -> Result<CiGrid, RostrumError> {
        self.fetch_ci(CiFetch::Explicit, None).await?;
        self.grid_now(filter).await
    }

    /// [`Self::refresh_ci`] for one watched repository.
    pub async fn refresh_ci_repo(
        &self,
        repo: String,
        filter: CiGridFilter,
    ) -> Result<CiGrid, RostrumError> {
        self.fetch_ci(CiFetch::Explicit, Some(parse_repo(&repo)?))
            .await?;
        self.grid_now(filter).await
    }

    /// The grid opening (on every watched repository, or `repo`): fetch
    /// only the checks that could have changed since the last fetch — never
    /// fetched, failed, a head moved, a pull request added, something
    /// running, or older than ten minutes. Skips quietly while GitHub has
    /// paused polling.
    pub async fn open_ci(
        &self,
        filter: CiGridFilter,
        repo: Option<String>,
    ) -> Result<CiGrid, RostrumError> {
        self.fetch_ci(CiFetch::Opened, repo_arg(repo)?).await?;
        self.grid_now(filter).await
    }

    /// The grid's 15 s watch while `any_running`: fetch only the
    /// repositories with anything queued or running. Skips quietly while
    /// GitHub has paused polling.
    pub async fn watch_ci(
        &self,
        filter: CiGridFilter,
        repo: Option<String>,
    ) -> Result<CiGrid, RostrumError> {
        self.fetch_ci(CiFetch::Watch, repo_arg(repo)?).await?;
        self.grid_now(filter).await
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
        let me = self
            .actor
            .call(move |state| {
                let marked = requeue(&mut state.ci, &flip, target);
                tracing::debug!(repo = %flip, ?target, marked, "checks marked queued");
                state.me.clone()
            })
            .await?;
        match client.rerun(&id, target).await {
            Ok(()) => {
                refetch_later(me, id, AFTER_RERUN);
                Ok(())
            }
            Err(error) => {
                tracing::info!(repo = %id, ?target, %error, "re-run refused");
                let only = BTreeSet::from([id.clone()]);
                if let Err(refetch) = fetch_checks(&me, CiFetch::Explicit, Some(only)).await {
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
