//! The poll loop: what each tick fetches, how long to wait before the next
//! one, and stopping altogether while GitHub refuses for rate limiting.
//!
//! The decisions are pure and live in `rostrum_core::{poll, budget}`; this
//! module feeds them the store's state and the client's budget, and carries
//! out the answer. See `docs/features/poll_budget.md`.

use std::time::Duration;

use chrono::Utc;
use gpui::Context;
use rostrum_core::{
    FeedTab,
    budget::{BudgetState, Feature, Pace, PaceReason, pace},
    poll::{CI_SAFETY_INTERVAL, Cadence, PollContext, PollDecision, Trigger, plan_poll},
};
use rostrum_github::batch::{PollShape, poll_cost_estimate};

use super::{Store, feed::FeedOrigin};

/// How often the points spent per feature are logged.
pub const SPEND_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The shortest wait between polls, whatever the budget says: a pause that
/// has just run out is re-checked after a moment, not in a tight loop.
const MIN_POLL_DELAY: Duration = Duration::from_secs(1);

impl Store {
    /// Refresh everything now, whatever is due: the Refresh action. Still
    /// nothing while GitHub has paused polling — every request would only be
    /// refused again.
    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.poll_tick(Trigger::Explicit, cx);
    }

    /// One poll: plan it and send what the plan names.
    pub(super) fn poll_tick(&mut self, trigger: Trigger, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let now = Utc::now();
        let budget = client.budget();
        let context = PollContext {
            now,
            trigger,
            cadence: self.cadence(),
            closed_shown: self.state.tab == FeedTab::Closed,
            pause: budget.active_pause(now),
        };
        match plan_poll(&self.state.repos, &context) {
            PollDecision::Paused(pause) => {
                tracing::info!(
                    until = %pause.until(),
                    cause = ?pause.cause(),
                    ?trigger,
                    "poll skipped: GitHub rate limit"
                );
            }
            PollDecision::Run(plan) => {
                tracing::debug!(
                    ?trigger,
                    feed = plan.feed.len(),
                    issues = plan.issues.len(),
                    closed = plan.closed.len(),
                    "poll"
                );
                self.cycle.begin(trigger);
                self.refresh_feed(plan.feed, FeedOrigin::Poll, cx);
                self.refresh_issues_batch(plan.issues, cx);
                for id in plan.closed {
                    self.refresh_closed(id, cx);
                }
            }
        }
        cx.notify();
    }

    /// The intervals the planner judges staleness by, from the config.
    pub(super) fn cadence(&self) -> Cadence {
        Cadence {
            issues: self.config.issue_refresh_interval(),
            ci_safety: CI_SAFETY_INTERVAL,
        }
    }

    /// Roughly what one poll costs, for pacing.
    fn poll_cost(&self) -> u32 {
        poll_cost_estimate(PollShape {
            repos: self.state.repos.len(),
            prs_per_repo: self.config.prs_per_repo,
            issues_per_repo: self.config.issues_per_repo,
            feed_secs: self.config.refresh_interval().as_secs(),
            issue_secs: self.config.issue_refresh_interval().as_secs(),
            ci_secs: CI_SAFETY_INTERVAL.as_secs(),
        })
    }

    /// How long to wait before the next poll: until a pause lifts, or the
    /// configured interval stretched as the budget runs down.
    fn next_poll_delay(&mut self) -> Duration {
        let now = Utc::now();
        let budget = self.client.as_ref().map(|client| client.budget());
        if let Some(pause) = budget.as_ref().and_then(|budget| budget.active_pause(now)) {
            return pause.remaining(now).max(MIN_POLL_DELAY);
        }
        let paced = pace(
            self.config.refresh_interval(),
            budget
                .as_ref()
                .and_then(|budget| budget.budgets.graphql.as_ref()),
            self.poll_cost(),
            now,
        );
        if paced.reason != PaceReason::Normal && self.pace != Some(paced) {
            tracing::info!(
                delay_secs = paced.delay.as_secs(),
                reason = ?paced.reason,
                remaining = budget
                    .as_ref()
                    .and_then(|budget| budget.budgets.graphql)
                    .map(|graphql| graphql.remaining()),
                "polling slowed to spare the GitHub budget"
            );
        }
        self.pace = Some(paced);
        paced.delay.max(MIN_POLL_DELAY)
    }

    pub(super) fn start_polling(&mut self, cx: &mut Context<Self>) {
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                // Errors here mean the store was dropped, so the loop ends.
                let Ok(delay) = this.update(cx, |this, _| this.next_poll_delay()) else {
                    break;
                };
                cx.background_executor().timer(delay).await;
                if this
                    .update(cx, |this, cx| this.poll_tick(Trigger::Timer, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        self.spend_log = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SPEND_LOG_INTERVAL).await;
                if this.update(cx, |this, _| this.log_spend()).is_err() {
                    break;
                }
            }
        }));
    }

    /// Log what each feature spent since the last report, and what is left.
    fn log_spend(&self) {
        let Some(client) = &self.client else {
            return;
        };
        let spent = client.take_spend();
        if spent.is_empty() {
            return;
        }
        let budget = client.budget();
        let points = |feature| spent.points(feature);
        tracing::info!(
            target: "rostrum::budget",
            minutes = SPEND_LOG_INTERVAL.as_secs() / 60,
            feed = points(Feature::Feed),
            issues = points(Feature::Issues),
            ci = points(Feature::Ci),
            closed = points(Feature::Closed),
            divergence = points(Feature::Divergence),
            probes = points(Feature::Probes),
            detail = points(Feature::Detail),
            graphql_points = spent.total_points(),
            graphql_requests = spent.total_graphql_requests(),
            stacks_rest = spent.rest_requests(Feature::Stacks),
            rest_requests = spent.total_rest_requests(),
            graphql_remaining = budget.budgets.graphql.map(|b| b.remaining()),
            core_remaining = budget.budgets.core.map(|b| b.remaining()),
            "GitHub budget spent"
        );
    }

    /// Whether GitHub has paused polling. Every background fetch asks first,
    /// so the CI view's watch and the merge probes stop too.
    pub(super) fn paused(&self, feature: Feature) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        let now = Utc::now();
        let budget = client.budget();
        let Some(pause) = budget.active_pause(now) else {
            return false;
        };
        tracing::debug!(feature = feature.label(), until = %pause.until(), "skipped: rate limit pause");
        true
    }

    /// The budget as the status bar shows it.
    pub fn budget(&self) -> Option<BudgetState> {
        self.client.as_ref().map(|client| client.budget())
    }

    /// The pace the poll loop chose last, when the budget made it stretch.
    pub fn pace(&self) -> Option<Pace> {
        self.pace.filter(|pace| pace.reason != PaceReason::Normal)
    }
}
