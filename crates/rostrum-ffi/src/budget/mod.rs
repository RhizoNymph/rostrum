//! The GitHub rate-limit budget on the phone: what is left, when the next
//! background refresh should run, and stopping every background fetch while
//! GitHub refuses for rate limiting.
//!
//! The rules are `rostrum_core::{budget, poll}`'s, the desktop's own
//! (`docs/features/poll_budget.md`); the client (`rostrum-github`) records
//! every response's budget, refusals and spend. Budgets are per token, so
//! each profile's core has its own.

mod spend;
mod types;

use std::time::Instant;

use chrono::{DateTime, Utc};
use rostrum_core::{
    budget::{BudgetState, Feature, Pause},
    poll::CI_SAFETY_INTERVAL,
};
use rostrum_github::{
    GitHubClient, GitHubError,
    batch::{PollShape, poll_cost_estimate},
};

pub(crate) use spend::SpendLog;
pub use types::{ApiBudget, ApiBudgetLevel, PauseReason, PollPace, PollSchedule, RateWindow};

use crate::{
    engine::{RostrumCore, state::CoreState},
    error::RostrumError,
    settings::refresh_interval,
};

/// Whether `error` is GitHub refusing for rate limiting. Such a refusal is
/// not the repository's fault: what it showed stays, and the budget readout
/// says why nothing moves.
pub(crate) fn is_rate_limit(error: &GitHubError) -> bool {
    matches!(
        error,
        GitHubError::RateLimited { .. } | GitHubError::SecondaryRateLimit { .. }
    )
}

/// The error an explicit request answers with while polling is paused:
/// nothing was sent, since GitHub would only refuse again.
pub(crate) fn paused_error(pause: &Pause) -> RostrumError {
    RostrumError::GitHubRateLimited {
        resets_at: pause.until().into(),
    }
}

impl CoreState {
    /// The session's client tagged with `feature` for the spend tally, or
    /// `NotSignedIn`.
    pub(crate) fn github_for(&self, feature: Feature) -> Result<GitHubClient, RostrumError> {
        Ok(self.github()?.for_feature(feature))
    }

    /// The budget the session's client has recorded; `None` when signed out.
    pub(crate) fn budget_state(&self) -> Option<BudgetState> {
        self.session.client().map(|client| client.budget())
    }

    /// The rate-limit pause in force at `now`, if any. Every background
    /// fetch asks first and sends nothing while one holds.
    pub(crate) fn active_pause(&self, now: DateTime<Utc>) -> Option<Pause> {
        self.budget_state()
            .and_then(|state| state.active_pause(now).copied())
    }

    /// [`Self::active_pause`], logged as the reason `feature` was skipped.
    pub(crate) fn paused_for(&self, feature: Feature) -> Option<Pause> {
        let pause = self.active_pause(Utc::now())?;
        tracing::debug!(
            feature = feature.label(),
            until = %pause.until(),
            "skipped: rate limit pause"
        );
        Some(pause)
    }

    /// Roughly what one background refresh costs, for pacing: the desktop's
    /// estimate over this profile's repositories and cadences.
    pub(crate) fn poll_cost(&self) -> u32 {
        poll_cost_estimate(PollShape {
            repos: self.feed.repos.len(),
            prs_per_repo: self.config.prs_per_repo.clamp(1, 100),
            issues_per_repo: self.config.issues_per_repo.clamp(1, 100),
            feed_secs: refresh_interval(&self.config).as_secs(),
            issue_secs: self.config.issue_refresh_interval().as_secs(),
            ci_secs: CI_SAFETY_INTERVAL.as_secs(),
        })
    }

    /// The budget as of `now`; `None` when signed out.
    pub(crate) fn api_budget(&self, now: DateTime<Utc>) -> Option<ApiBudget> {
        let state = self.budget_state()?;
        Some(types::api_budget(
            refresh_interval(&self.config),
            &state,
            self.poll_cost(),
            now,
        ))
    }

    /// When the next background refresh should run, logging when the budget
    /// starts (or stops) stretching it.
    pub(crate) fn poll_schedule(&mut self, now: DateTime<Utc>) -> PollSchedule {
        let base = refresh_interval(&self.config);
        let state = self.budget_state().unwrap_or_default();
        let schedule = types::schedule(base, &state, self.poll_cost(), now);
        if self.last_pace.as_ref() != Some(&schedule.pace) {
            match &schedule.pace {
                PollPace::Normal => {
                    if self.last_pace.is_some() {
                        tracing::info!(
                            delay_secs = schedule.delay_secs,
                            "polling back to its interval"
                        );
                    }
                }
                pace => tracing::info!(
                    delay_secs = schedule.delay_secs,
                    base_secs = schedule.base_secs,
                    ?pace,
                    graphql_remaining = state.budgets.graphql.map(|budget| budget.remaining()),
                    "polling slowed to spare the GitHub budget"
                ),
            }
            self.last_pace = Some(schedule.pace.clone());
        }
        schedule
    }

    /// Log what each feature spent, when [`spend::SPEND_LOG_INTERVAL`] has
    /// passed since the last report.
    pub(crate) fn log_spend_if_due(&mut self, now: Instant) {
        if !self.spend_log.due(now) {
            return;
        }
        let Some(client) = self.session.client() else {
            return;
        };
        self.spend_log.logged(now);
        spend::log(&client.take_spend(), &client.budget());
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// This profile's GitHub budget as of now: what is left of each hourly
    /// window and when the next background refresh runs (or until when
    /// GitHub has paused it). No network. `None` when signed out.
    pub async fn api_budget(&self) -> Result<Option<ApiBudget>, RostrumError> {
        self.actor.call(|state| state.api_budget(Utc::now())).await
    }

    /// How long to wait before the next foreground refresh: the configured
    /// interval, stretched as the budget runs down, or until a rate-limit
    /// pause lifts. Call before each wait; no network.
    pub async fn poll_schedule(&self) -> Result<PollSchedule, RostrumError> {
        self.actor
            .call(|state| state.poll_schedule(Utc::now()))
            .await
    }
}
