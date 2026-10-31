//! The rate-limit budget as Kotlin sees it, and the pure conversions from
//! `rostrum_core::budget`.
//!
//! Everything is evaluated against an explicit `now`: a window that has
//! reset reads as full (and has no reset time), and a pause that has run
//! out is no pause.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use rostrum_core::budget::{
    BudgetLevel, BudgetState, Pace, PaceReason, Pause, PauseCause, RateBudget,
};

/// How much of a budget is left, by the desktop's thresholds
/// (`rostrum_core::budget::{LOW_FRACTION, CRITICAL_FRACTION}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ApiBudgetLevel {
    /// 20% or more left.
    Healthy,
    /// Under 20%: polling slows down.
    Low,
    /// Under 10%: polling slows down further.
    Critical,
    /// Nothing left until the reset.
    Exhausted,
}

impl From<BudgetLevel> for ApiBudgetLevel {
    fn from(level: BudgetLevel) -> Self {
        match level {
            BudgetLevel::Healthy => Self::Healthy,
            BudgetLevel::Low => Self::Low,
            BudgetLevel::Critical => Self::Critical,
            BudgetLevel::Exhausted => Self::Exhausted,
        }
    }
}

/// One hourly budget (GraphQL points, or REST requests) as of now.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RateWindow {
    /// Left in this window; the whole limit once the window has reset.
    /// Never above `limit`.
    pub remaining: u32,
    /// At least 1.
    pub limit: u32,
    /// When the window refills; `None` once it has (the budget is full).
    pub resets_at: Option<SystemTime>,
    pub level: ApiBudgetLevel,
}

impl RateWindow {
    pub(crate) fn of(budget: &RateBudget, now: DateTime<Utc>) -> Self {
        Self {
            remaining: budget.remaining_at(now),
            limit: budget.limit(),
            resets_at: (!budget.has_reset(now)).then(|| budget.reset_at().into()),
            level: budget.level_at(now).into(),
        }
    }
}

/// Which limit GitHub refused for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PauseReason {
    /// The hourly budget is spent; it refills at the reset.
    PrimaryLimit,
    /// Too many requests too quickly; GitHub said how long to wait.
    SecondaryLimit,
}

impl From<PauseCause> for PauseReason {
    fn from(cause: PauseCause) -> Self {
        match cause {
            PauseCause::Primary => Self::PrimaryLimit,
            PauseCause::Secondary => Self::SecondaryLimit,
        }
    }
}

/// Why the next background refresh waits as long as it does.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PollPace {
    /// The configured interval.
    Normal,
    /// Stretched because the GraphQL budget is `level` (never `Healthy`).
    Stretched { level: ApiBudgetLevel },
    /// Nothing usable is left before the reset: wait for it.
    UntilReset,
    /// GitHub refused for rate limiting: nothing is sent until `until`.
    Paused {
        until: SystemTime,
        reason: PauseReason,
    },
}

/// When the next background refresh should run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PollSchedule {
    /// Seconds until the next refresh; at least 1.
    pub delay_secs: u64,
    /// The configured interval, for "slowed from 1 min to 4 min".
    pub base_secs: u64,
    pub pace: PollPace,
}

/// The GitHub token's rate-limit budget: per token, so shared with anything
/// else using it (the paired desktop and `gh`, when the token came from
/// there).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ApiBudget {
    /// GraphQL points; `None` until a response has reported them.
    pub graphql: Option<RateWindow>,
    /// REST requests; `None` until a response has reported them.
    pub rest: Option<RateWindow>,
    /// The next background refresh, including any pause in force.
    pub schedule: PollSchedule,
}

/// The shortest wait between polls, whatever the budget says: a pause that
/// has just run out is re-checked after a moment, not in a tight loop.
pub(crate) const MIN_POLL_DELAY: Duration = Duration::from_secs(1);

/// Whole seconds, rounded up, at least [`MIN_POLL_DELAY`].
fn delay_secs(delay: Duration) -> u64 {
    let delay = delay.max(MIN_POLL_DELAY);
    delay.as_secs() + u64::from(delay.subsec_nanos() > 0)
}

impl PollSchedule {
    /// The wait a pause in force calls for.
    pub(crate) fn paused(pause: &Pause, base: Duration, now: DateTime<Utc>) -> Self {
        Self {
            delay_secs: delay_secs(pause.remaining(now)),
            base_secs: base.as_secs(),
            pace: PollPace::Paused {
                until: pause.until().into(),
                reason: pause.cause().into(),
            },
        }
    }

    /// The wait [`rostrum_core::budget::pace`] chose.
    pub(crate) fn paced(pace: Pace, base: Duration) -> Self {
        Self {
            delay_secs: delay_secs(pace.delay),
            base_secs: base.as_secs(),
            pace: match pace.reason {
                PaceReason::Normal => PollPace::Normal,
                PaceReason::Stretched { level } => PollPace::Stretched {
                    level: level.into(),
                },
                PaceReason::UntilReset => PollPace::UntilReset,
            },
        }
    }

    /// The pause in force, if this schedule is one.
    pub fn paused_until(&self) -> Option<SystemTime> {
        match self.pace {
            PollPace::Paused { until, .. } => Some(until),
            _ => None,
        }
    }
}

/// When to poll next: until a pause lifts, or `base` stretched as the
/// GraphQL budget runs down at `cost` points a poll.
pub(crate) fn schedule(
    base: Duration,
    state: &BudgetState,
    cost: u32,
    now: DateTime<Utc>,
) -> PollSchedule {
    if let Some(pause) = state.active_pause(now) {
        return PollSchedule::paused(pause, base, now);
    }
    PollSchedule::paced(
        rostrum_core::budget::pace(base, state.budgets.graphql.as_ref(), cost, now),
        base,
    )
}

/// The budget as Kotlin shows it.
pub(crate) fn api_budget(
    base: Duration,
    state: &BudgetState,
    cost: u32,
    now: DateTime<Utc>,
) -> ApiBudget {
    ApiBudget {
        graphql: state
            .budgets
            .graphql
            .as_ref()
            .map(|budget| RateWindow::of(budget, now)),
        rest: state
            .budgets
            .core
            .as_ref()
            .map(|budget| RateWindow::of(budget, now)),
        schedule: schedule(base, state, cost, now),
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::budget::{Jitter, RateLimitHit, RateResource};

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    fn system(secs: i64) -> SystemTime {
        at(secs).into()
    }

    const BASE: Duration = Duration::from_secs(60);

    fn with_graphql(remaining: u32, reset: i64) -> BudgetState {
        let mut state = BudgetState::default();
        state.observe(
            RateResource::Graphql,
            RateBudget::new(5000, remaining, at(reset)).expect("budget"),
        );
        state
    }

    #[test]
    fn nothing_observed_is_no_window_and_the_configured_interval() {
        let budget = api_budget(BASE, &BudgetState::default(), 5, at(0));
        assert_eq!(budget.graphql, None);
        assert_eq!(budget.rest, None);
        assert_eq!(
            budget.schedule,
            PollSchedule {
                delay_secs: 60,
                base_secs: 60,
                pace: PollPace::Normal
            }
        );
    }

    #[test]
    fn a_window_reads_as_of_now() {
        let state = with_graphql(4123, 600);
        let window = api_budget(BASE, &state, 5, at(0)).graphql.expect("window");
        assert_eq!(
            window,
            RateWindow {
                remaining: 4123,
                limit: 5000,
                resets_at: Some(system(600)),
                level: ApiBudgetLevel::Healthy,
            }
        );
    }

    #[test]
    fn a_window_that_has_reset_is_full_with_no_reset_time() {
        let state = with_graphql(0, 600);
        let window = api_budget(BASE, &state, 5, at(600))
            .graphql
            .expect("window");
        assert_eq!(window.remaining, 5000);
        assert_eq!(window.resets_at, None);
        assert_eq!(window.level, ApiBudgetLevel::Healthy);
    }

    #[test]
    fn levels_follow_the_desktop_thresholds() {
        let level = |remaining| {
            api_budget(BASE, &with_graphql(remaining, 600), 5, at(0))
                .graphql
                .expect("window")
                .level
        };
        assert_eq!(level(1000), ApiBudgetLevel::Healthy);
        assert_eq!(level(999), ApiBudgetLevel::Low);
        assert_eq!(level(499), ApiBudgetLevel::Critical);
        assert_eq!(level(0), ApiBudgetLevel::Exhausted);
    }

    #[test]
    fn rest_is_reported_apart_from_graphql() {
        let mut state = BudgetState::default();
        state.observe(
            RateResource::Core,
            RateBudget::new(5000, 4990, at(900)).expect("budget"),
        );
        let budget = api_budget(BASE, &state, 5, at(0));
        assert_eq!(budget.graphql, None);
        assert_eq!(budget.rest.map(|window| window.remaining), Some(4990));
    }

    #[test]
    fn a_low_budget_stretches_the_interval() {
        // 800 left, 100 held back: 700 usable at 5 a poll is 140 polls over
        // 3600 s, about 26 s each, so the 2x floor wins.
        let schedule = schedule(BASE, &with_graphql(800, 3600), 5, at(0));
        assert_eq!(schedule.delay_secs, 120);
        assert_eq!(
            schedule.pace,
            PollPace::Stretched {
                level: ApiBudgetLevel::Low
            }
        );
    }

    #[test]
    fn nothing_usable_waits_for_the_reset() {
        let schedule = schedule(BASE, &with_graphql(100, 900), 5, at(0));
        assert_eq!(schedule.delay_secs, 900);
        assert_eq!(schedule.pace, PollPace::UntilReset);
    }

    #[test]
    fn a_pause_waits_until_it_lifts_and_says_why() {
        let mut state = with_graphql(4000, 3600);
        state.hit(
            RateLimitHit::Secondary {
                retry_after: Duration::from_secs(60),
            },
            at(0),
            Jitter::new(Duration::from_secs(10)),
        );
        let schedule = schedule(BASE, &state, 5, at(30));
        assert_eq!(schedule.delay_secs, 40);
        assert_eq!(
            schedule.pace,
            PollPace::Paused {
                until: system(70),
                reason: PauseReason::SecondaryLimit
            }
        );
        assert_eq!(schedule.paused_until(), Some(system(70)));
        // Once it has run out, the budget paces as usual.
        let after = super::schedule(BASE, &state, 5, at(70));
        assert_eq!(after.pace, PollPace::Normal);
        assert_eq!(after.paused_until(), None);
    }

    #[test]
    fn delays_round_up_to_whole_seconds_and_never_reach_zero() {
        assert_eq!(delay_secs(Duration::ZERO), 1);
        assert_eq!(delay_secs(Duration::from_millis(1500)), 2);
        assert_eq!(delay_secs(Duration::from_secs(60)), 60);
    }
}
