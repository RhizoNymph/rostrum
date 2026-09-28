//! Fetching the feed: every repository a few at a time, each with its
//! distance from base, and background re-checks of merge states GitHub is
//! still computing.
//!
//! The network happens outside the state actor. The actor hands out a plan
//! (client, limit, and a sequence number per repository), each fetch's result
//! is applied back through it as it lands, and a fetch that lands after a
//! newer one for the same repository is dropped.

use std::time::Duration;

use futures::{StreamExt, stream};
use rostrum_core::{
    MergeProbeBudget, RepoId, apply_divergences, divergence_query, needs_merge_probe,
};
use rostrum_github::{GitHubClient, GitHubError};
use tokio::task::AbortHandle;

use crate::{
    engine::{RostrumCore, actor::WeakActor, state::CoreState, writer::Write},
    error::RostrumError,
    feed::{
        FeedSnapshot,
        state::{Applied, Fetched},
    },
};

/// Repositories fetched at once. GitHub's secondary rate limit punishes
/// bursts of concurrent requests; four keeps a thirty-repository refresh
/// quick without tripping it.
const CONCURRENT_FETCHES: usize = 4;

/// Whether a refresh may start background merge-state re-checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Probes {
    /// Foreground: re-check, and deliver the results to the observer.
    Schedule,
    /// Background notification check: nobody is watching the feed.
    Skip,
}

/// Which repositories a refresh covers.
pub(crate) enum Scope {
    /// Every watched repository; starts a new probe cycle.
    All,
    /// Every watched repository, leaving any foreground probe cycle alone —
    /// the background notification check.
    Background,
    /// Just one.
    One(RepoId),
}

/// One repository's merge-state re-check budget, and the re-check in flight.
#[derive(Default)]
pub(crate) struct ProbeSlot {
    budget: MergeProbeBudget,
    task: Option<AbortHandle>,
}

impl ProbeSlot {
    /// A re-check is scheduled or running.
    pub(crate) fn is_pending(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }
}

impl Drop for ProbeSlot {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// What a refresh needs from the state to run outside it.
pub(crate) struct Plan {
    client: GitHubClient,
    limit: u32,
    fetches: Vec<(RepoId, u64)>,
}

/// Fetch one repository's open pull requests and, in a second request, how
/// far each is from its base. A failed divergence batch is not a failed
/// fetch: the counts carry forward from the previous refresh instead.
pub(crate) async fn fetch_repo(
    client: &GitHubClient,
    repo: &RepoId,
    limit: u32,
) -> Result<Fetched, GitHubError> {
    let mut fetched = client.open_pull_requests(repo, limit).await?;
    let (numbers, pairs) = divergence_query(&fetched.pull_requests);
    match client.divergences(repo, &pairs).await {
        Ok(divergences) => apply_divergences(&mut fetched.pull_requests, &numbers, divergences),
        Err(error) => {
            tracing::debug!(%repo, %error, "divergence batch failed; keeping previous counts");
        }
    }
    if let Some(limit) = &fetched.rate_limit {
        tracing::debug!(%repo, cost = limit.cost, remaining = limit.remaining, "fetched");
    }
    Ok(Fetched {
        prs: fetched.pull_requests,
        viewer: fetched.viewer,
    })
}

impl CoreState {
    pub(crate) fn begin_refresh(&mut self, scope: &Scope) -> Result<Plan, RostrumError> {
        let client = self.github()?;
        let ids = match scope {
            Scope::All => {
                // A full refresh starts a new cycle: every repository gets its
                // probe budget back, and in-flight re-checks are redundant.
                self.probes.clear();
                self.feed.repo_ids()
            }
            Scope::Background => self.feed.repo_ids(),
            Scope::One(id) => vec![id.clone()],
        };
        let fetches: Vec<(RepoId, u64)> = ids
            .into_iter()
            .filter_map(|id| self.feed.begin_fetch(&id).map(|seq| (id, seq)))
            .collect();
        if let Scope::One(id) = scope
            && fetches.is_empty()
        {
            return Err(RostrumError::invalid(format!("{id} is not watched")));
        }
        self.publish();
        Ok(Plan {
            client,
            limit: self.config.prs_per_repo.clamp(1, 100),
            fetches,
        })
    }

    /// Apply one repository's fetch: the list, the viewer it reported, the
    /// cache write, and what a failure says about the token.
    pub(crate) fn apply_fetch(
        &mut self,
        repo: &RepoId,
        seq: u64,
        outcome: Result<Fetched, GitHubError>,
    ) -> Applied {
        match outcome {
            Ok(fetched) => {
                let viewer = fetched.viewer.clone();
                let applied = self.feed.apply_fetch(repo, seq, Ok(fetched));
                if applied == Applied::Loaded {
                    if let Some(viewer) = viewer {
                        self.session.verify(viewer);
                    }
                    if let Some(state) = self.feed.repos.iter().find(|state| &state.id == repo) {
                        self.writer.send(Write::PullRequests {
                            repo: repo.clone(),
                            prs: state.prs.clone(),
                        });
                    }
                }
                applied
            }
            Err(error) => {
                tracing::warn!(%repo, %error, "refresh failed");
                self.note_github_error(&error);
                self.feed.apply_fetch(repo, seq, Err(&error))
            }
        }
    }

    /// Start a merge-state re-check for each of `repos` that is waiting on
    /// GitHub's computation and still has budget this cycle.
    pub(crate) fn schedule_probes(&mut self, repos: &[RepoId], client: &GitHubClient, limit: u32) {
        for repo in repos {
            let computing = self
                .feed
                .repos
                .iter()
                .find(|state| &state.id == repo)
                .is_some_and(|state| needs_merge_probe(&state.prs));
            if !computing {
                self.probes.remove(repo);
                continue;
            }
            let slot = self.probes.entry(repo.clone()).or_default();
            let Some(delay) = slot.budget.next_delay() else {
                // The steady state for a token that may not read merge state:
                // quiet, until the next full refresh.
                tracing::debug!(%repo, "merge state still unknown; waiting for the next refresh");
                continue;
            };
            tracing::debug!(%repo, attempt = slot.budget.spent(), delay_ms = delay.as_millis(), "scheduling a merge-state re-check");
            let task = tokio::spawn(probe(
                self.me.clone(),
                client.clone(),
                repo.clone(),
                limit,
                delay,
            ));
            if let Some(previous) = slot.task.replace(task.abort_handle()) {
                previous.abort();
            }
        }
    }
}

/// One merge-state re-check: wait, fetch, apply, and schedule the next if
/// GitHub is still computing. Holds no strong handle on the core while it
/// waits or fetches, so it dies with the core.
async fn probe(me: WeakActor, client: GitHubClient, repo: RepoId, limit: u32, delay: Duration) {
    tokio::time::sleep(delay).await;
    let Some(actor) = me.upgrade() else {
        return;
    };
    let id = repo.clone();
    let Ok(Some(seq)) = actor.call(move |state| state.feed.begin_fetch(&id)).await else {
        return;
    };
    drop(actor);

    let outcome = fetch_repo(&client, &repo, limit).await;

    let Some(actor) = me.upgrade() else {
        return;
    };
    let _ = actor
        .call(move |state| {
            // This re-check is finished; only a new one counts as pending.
            if let Some(slot) = state.probes.get_mut(&repo) {
                slot.task = None;
            }
            state.apply_fetch(&repo, seq, outcome);
            state.schedule_probes(std::slice::from_ref(&repo), &client, limit);
            state.publish();
        })
        .await;
}

impl RostrumCore {
    /// Fill the feed from SQLite, once. No network.
    pub(crate) async fn ensure_hydrated(&self) -> Result<(), RostrumError> {
        let pending = self
            .actor
            .call(|state| (!state.feed.hydrated).then(|| state.feed.repo_ids()))
            .await?;
        let Some(ids) = pending else {
            return Ok(());
        };
        let mut cached = Vec::new();
        for id in ids {
            match self.db.load_pull_requests(&id).await {
                Ok(prs) if !prs.is_empty() => cached.push((id, prs)),
                Ok(_) => {}
                Err(error) => tracing::warn!(repo = %id, %error, "could not read the cache"),
            }
        }
        self.actor
            .call(move |state| {
                if !state.feed.hydrated {
                    state.feed.hydrate(cached);
                    state.publish();
                }
            })
            .await
    }

    /// Fetch `scope` and apply each repository as it lands. Fails only when
    /// there is no token or GitHub rejected it; any other per-repository
    /// failure is in that repository's section.
    pub(crate) async fn refresh(
        &self,
        scope: Scope,
        probes: Probes,
    ) -> Result<FeedSnapshot, RostrumError> {
        self.ensure_hydrated().await?;
        let plan = self
            .actor
            .try_call(move |state| state.begin_refresh(&scope))
            .await?;
        let Plan {
            client,
            limit,
            fetches,
        } = plan;
        let repos: Vec<RepoId> = fetches.iter().map(|(repo, _)| repo.clone()).collect();

        let fetching = client.clone();
        let mut landing = stream::iter(fetches)
            .map(|(repo, seq)| {
                let client = fetching.clone();
                async move {
                    let outcome = fetch_repo(&client, &repo, limit).await;
                    (repo, seq, outcome)
                }
            })
            .buffer_unordered(CONCURRENT_FETCHES);

        let mut rejected = None;
        while let Some((repo, seq, outcome)) = landing.next().await {
            if let Err(error @ GitHubError::Unauthorized) = &outcome {
                rejected = Some(error.to_string());
            }
            self.actor
                .call(move |state| {
                    state.apply_fetch(&repo, seq, outcome);
                    state.publish();
                })
                .await?;
        }
        drop(landing);

        let snapshot = self
            .actor
            .call(move |state| {
                if probes == Probes::Schedule {
                    state.schedule_probes(&repos, &client, limit);
                }
                state.publish()
            })
            .await?;
        match rejected {
            Some(reason) => Err(RostrumError::GitHubAuthFailed { reason }),
            None => Ok(snapshot),
        }
    }
}
