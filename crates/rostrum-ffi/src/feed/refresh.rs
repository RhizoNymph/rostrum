//! Refreshing the feed: plan what is due (`rostrum_core::poll::plan_poll`,
//! the desktop's planner), send it in batches, and once the pull requests
//! have landed, follow up with one divergence batch, the stacks, and the
//! cycle's merge probe.
//!
//! The network happens outside the state actor. The actor hands out a plan
//! (client, limits, and a sequence number per repository), each batch is
//! applied back through it as it lands, and a fetch that lands after a newer
//! one for the same repository is dropped. Nothing is sent while GitHub has
//! paused polling for rate limiting.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use rostrum_core::{
    RepoId,
    budget::{Feature, Pause},
    poll::{CI_SAFETY_INTERVAL, Cadence, PollContext, PollDecision, Trigger, plan_poll},
};
use rostrum_github::GitHubClient;

use crate::{
    budget::paused_error,
    engine::{RostrumCore, state::CoreState},
    error::RostrumError,
    feed::{
        FeedSnapshot,
        batch::Seq,
        land::{land_divergences, land_issues, land_pulls, land_stacks},
        state::Cached,
    },
};

/// Which repositories a refresh covers, and why.
#[derive(Clone, Debug)]
pub(crate) enum Scope {
    /// Every watched repository: pull requests, the issues that are due (all
    /// of them for [`Trigger::Explicit`]), and the follow-ups. Starts a new
    /// merge-probe cycle.
    Poll(Trigger),
    /// The background notification check: pull requests alone, no
    /// follow-ups, leaving any foreground probe cycle alone.
    Background,
    /// One repository, pull requests and issues, e.g. after adding it or
    /// after a change made from the phone.
    One(RepoId),
}

impl Scope {
    /// Whether divergence, stacks and probes follow the pull requests.
    fn follows_up(&self) -> bool {
        !matches!(self, Self::Background)
    }
}

/// After GitHub says a repository has no stacked pull requests, it is not
/// asked again for this long, as the desktop does.
const STACKS_UNAVAILABLE_FOR: Duration = Duration::from_secs(60 * 60);

/// What a refresh needs from the state to run outside it.
pub(crate) struct Plan {
    client: GitHubClient,
    limit: u32,
    issue_limit: u32,
    follow_ups: bool,
    /// Repositories whose stacks GitHub recently said are not enabled.
    skip_stacks: HashSet<RepoId>,
    pulls: Vec<Seq>,
    issues: Vec<RepoId>,
}

/// What [`CoreState::begin_refresh`] decided.
pub(crate) enum Begun {
    /// GitHub has paused polling: nothing is sent.
    Paused(Pause),
    Run(Plan),
}

/// What a refresh did.
pub(crate) enum Refreshed {
    Fetched(FeedSnapshot),
    /// Nothing was sent: GitHub has paused polling until `pause` lifts.
    Paused {
        pause: Pause,
        snapshot: FeedSnapshot,
    },
}

impl Refreshed {
    /// The snapshot either way: a timer's refresh skipping a pause is not a
    /// failure (the snapshot's budget says why nothing moved).
    pub(crate) fn snapshot(self) -> FeedSnapshot {
        match self {
            Self::Fetched(snapshot) | Self::Paused { snapshot, .. } => snapshot,
        }
    }

    /// The snapshot, or for a refresh the user asked for, the pause as
    /// `GitHubRateLimited`.
    pub(crate) fn asked(self) -> Result<FeedSnapshot, RostrumError> {
        match self {
            Self::Fetched(snapshot) => Ok(snapshot),
            Self::Paused { pause, .. } => Err(paused_error(&pause)),
        }
    }
}

impl CoreState {
    /// The intervals the planner judges staleness by.
    fn cadence(&self) -> Cadence {
        Cadence {
            issues: self.config.issue_refresh_interval(),
            ci_safety: CI_SAFETY_INTERVAL,
        }
    }

    /// Plan a refresh of `scope` at `now` and mark what it fetches as
    /// loading.
    pub(crate) fn begin_refresh(
        &mut self,
        scope: &Scope,
        now: DateTime<Utc>,
    ) -> Result<Begun, RostrumError> {
        let client = self.github()?;
        if let Scope::One(id) = scope
            && self.feed.repo(id).is_none()
        {
            return Err(RostrumError::invalid(format!("{id} is not watched")));
        }
        let budget = client.budget();
        let pause = budget.active_pause(now);
        let planned = match scope {
            Scope::Poll(trigger) => self.planned(*trigger, now, pause),
            Scope::Background => self
                .planned(Trigger::Timer, now, pause)
                .map(|(pulls, _)| (pulls, Vec::new())),
            Scope::One(id) => match pause {
                Some(pause) => Err(*pause),
                None => Ok((vec![id.clone()], vec![id.clone()])),
            },
        };
        let (pulls, issues) = match planned {
            Ok(planned) => planned,
            Err(pause) => {
                tracing::info!(until = %pause.until(), cause = ?pause.cause(), ?scope, "refresh skipped: GitHub rate limit");
                return Ok(Begun::Paused(pause));
            }
        };
        for repo in &issues {
            self.feed.begin_issues(repo);
        }
        let pulls: Vec<Seq> = pulls
            .into_iter()
            .filter_map(|id| self.feed.begin_fetch(&id).map(|seq| (id, seq)))
            .collect();
        if matches!(scope, Scope::Poll(_)) {
            // A refresh of everything starts a new cycle: the probe
            // allowance comes back, and a probe in flight is redundant.
            self.probe.restart();
        }
        tracing::debug!(
            ?scope,
            pulls = pulls.len(),
            issues = issues.len(),
            "refresh"
        );
        self.publish();
        let held = Instant::now();
        self.stacks_unavailable
            .retain(|_, since| held.duration_since(*since) < STACKS_UNAVAILABLE_FOR);
        Ok(Begun::Run(Plan {
            client,
            limit: self.config.prs_per_repo.clamp(1, 100),
            issue_limit: self.config.issues_per_repo.clamp(1, 100),
            follow_ups: scope.follows_up(),
            skip_stacks: self.stacks_unavailable.keys().cloned().collect(),
            pulls,
            issues,
        }))
    }

    /// `plan_poll` over every watched repository: the pull requests, and the
    /// issues that are due; or the pause in force.
    fn planned(
        &self,
        trigger: Trigger,
        now: DateTime<Utc>,
        pause: Option<&Pause>,
    ) -> Result<(Vec<RepoId>, Vec<RepoId>), Pause> {
        let context = PollContext {
            now,
            trigger,
            cadence: self.cadence(),
            closed_shown: false,
            pause,
        };
        match plan_poll(&self.feed.repos, &context) {
            PollDecision::Paused(pause) => Err(pause),
            PollDecision::Run(plan) => Ok((plan.feed, plan.issues)),
        }
    }
}

impl RostrumCore {
    /// Re-read one repository's open issues and apply them, after an issue
    /// changed. Pull requests are left alone; nothing is sent while GitHub
    /// has paused polling.
    pub(crate) async fn refresh_issues(&self, repo: &RepoId) -> Result<(), RostrumError> {
        let planned = self
            .actor
            .try_call(|state| {
                let client = state.github_for(Feature::Issues)?;
                Ok(state
                    .paused_for(Feature::Issues)
                    .is_none()
                    .then(|| (client, state.config.issues_per_repo.clamp(1, 100))))
            })
            .await?;
        let Some((client, limit)) = planned else {
            return Ok(());
        };
        land_issues(&self.actor.downgrade(), &client, vec![repo.clone()], limit).await
    }

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
            let loaded = Cached {
                prs: self
                    .db
                    .load_pull_requests(&id)
                    .await
                    .unwrap_or_else(|error| {
                        tracing::warn!(repo = %id, %error, "could not read cached pull requests");
                        Vec::new()
                    }),
                issues: self.db.load_issues(&id).await.unwrap_or_else(|error| {
                    tracing::warn!(repo = %id, %error, "could not read cached issues");
                    Vec::new()
                }),
                stacks: self.db.load_stacks(&id).await.unwrap_or_else(|error| {
                    tracing::warn!(repo = %id, %error, "could not read cached stacks");
                    Vec::new()
                }),
                meta: self.db.load_repo_meta(&id).await.unwrap_or_else(|error| {
                    tracing::warn!(repo = %id, %error, "could not read cached repository facts");
                    None
                }),
            };
            cached.push((id, loaded));
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

    /// Refresh `scope`: plan it, send its batches and apply each as it
    /// lands, then the follow-ups. Fails only when there is no token or
    /// GitHub rejected it; any other per-repository failure is in that
    /// repository's section.
    pub(crate) async fn refresh(&self, scope: Scope) -> Result<Refreshed, RostrumError> {
        self.ensure_hydrated().await?;
        let begun = self
            .actor
            .try_call(move |state| state.begin_refresh(&scope, Utc::now()))
            .await?;
        let plan = match begun {
            Begun::Run(plan) => plan,
            Begun::Paused(pause) => {
                let snapshot = self.actor.call(|state| state.publish()).await?;
                return Ok(Refreshed::Paused { pause, snapshot });
            }
        };
        let Plan {
            client,
            limit,
            issue_limit,
            follow_ups,
            skip_stacks,
            pulls,
            issues,
        } = plan;
        let me = self.actor.downgrade();
        let feed = client.for_feature(Feature::Feed);
        let issue_client = client.for_feature(Feature::Issues);
        let (landed, issues_landed) = tokio::join!(
            land_pulls(&me, &feed, pulls, limit),
            land_issues(&me, &issue_client, issues, issue_limit),
        );
        let landed = landed?;
        issues_landed?;

        if follow_ups && !landed.landed.is_empty() {
            let divergence = client.for_feature(Feature::Divergence);
            let stacks = client.for_feature(Feature::Stacks);
            let (diverged, stacked) = tokio::join!(
                land_divergences(&me, &divergence, landed.landed.clone(), limit),
                land_stacks(&me, &stacks, landed.landed, skip_stacks),
            );
            diverged?;
            stacked?;
        }

        let snapshot = self
            .actor
            .call(move |state| {
                if follow_ups {
                    state.schedule_probe(&client, limit);
                }
                state.log_spend_if_due(Instant::now());
                state.publish()
            })
            .await?;
        match landed.rejected {
            Some(reason) => Err(RostrumError::GitHubAuthFailed { reason }),
            None => Ok(Refreshed::Fetched(snapshot)),
        }
    }
}
