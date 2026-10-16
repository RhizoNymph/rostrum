//! GitHub's rate-limit budget, and what polling does about it.
//!
//! The desktop app, the `gh` CLI and a paired phone usually share one token,
//! and so one GraphQL budget (5,000 points an hour) and one REST "core"
//! budget. Everything here is pure — the client feeds observations in, the
//! poller reads decisions out — so the desktop store and the Android core
//! pace themselves by the same rules. See `docs/features/poll_budget.md`.
//!
//! - [`RateBudget`] / [`Budgets`]: what GitHub last said is left, per
//!   [`RateResource`].
//! - [`Pause`]: polling stopped after GitHub refused for rate limiting,
//!   primary or secondary, and when it resumes.
//! - [`pace`]: how far to stretch the poll interval as the budget runs down.
//! - [`SpendTally`]: points and requests spent per [`Feature`], for the
//!   periodic log line.
//! - [`BudgetState`]: the three together, as the client's tracker holds them.

mod pace;
mod pause;
mod rate;
mod spend;

pub use pace::{Pace, PaceReason, RESERVE_FRACTION, pace, reserve};
pub use pause::{
    Jitter, MAX_RESUME_JITTER, MIN_RESUME_JITTER, Pause, PauseCause, RateLimitHit,
    SECONDARY_DEFAULT_RETRY,
};
pub use rate::{BudgetLevel, Budgets, CRITICAL_FRACTION, LOW_FRACTION, RateBudget, RateResource};
pub use spend::{Feature, SpendTally};

use chrono::{DateTime, Utc};

/// Everything known about the budget: the last observation per resource,
/// any pause in force, and what has been spent since the last report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BudgetState {
    pub budgets: Budgets,
    pause: Option<Pause>,
    pub spend: SpendTally,
}

impl BudgetState {
    /// A response reported `budget` for `resource`.
    pub fn observe(&mut self, resource: RateResource, budget: RateBudget) {
        self.budgets.observe(resource, budget);
    }

    /// GitHub refused a request for rate limiting. Polling pauses until the
    /// limit lifts; a pause already in force is only ever lengthened.
    pub fn hit(&mut self, hit: RateLimitHit, now: DateTime<Utc>, jitter: Jitter) {
        let pause = Pause::after(hit, now, jitter);
        self.pause = Some(match self.pause.take() {
            Some(held) if held.is_active(now) => held.longer(pause),
            _ => pause,
        });
    }

    /// The pause in force at `now`, if any. One that has run out is no pause.
    pub fn active_pause(&self, now: DateTime<Utc>) -> Option<&Pause> {
        self.pause.as_ref().filter(|pause| pause.is_active(now))
    }

    /// Hand over the tally and start a new one.
    pub fn take_spend(&mut self) -> SpendTally {
        std::mem::take(&mut self.spend)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    #[test]
    fn no_pause_until_github_refuses() {
        let state = BudgetState::default();
        assert!(state.active_pause(at(0)).is_none());
    }

    #[test]
    fn a_primary_limit_pauses_until_reset_plus_jitter() {
        let mut state = BudgetState::default();
        state.hit(
            RateLimitHit::Primary { reset_at: at(600) },
            at(0),
            Jitter::new(Duration::from_secs(10)),
        );
        let pause = state.active_pause(at(0)).expect("paused");
        assert_eq!(pause.until(), at(610));
        assert_eq!(pause.cause(), PauseCause::Primary);
        assert!(state.active_pause(at(609)).is_some());
        assert!(state.active_pause(at(610)).is_none(), "resumes at until");
    }

    #[test]
    fn a_shorter_hit_never_shortens_a_pause_in_force() {
        let mut state = BudgetState::default();
        let jitter = Jitter::new(Duration::from_secs(5));
        state.hit(RateLimitHit::Primary { reset_at: at(900) }, at(0), jitter);
        state.hit(
            RateLimitHit::Secondary {
                retry_after: Duration::from_secs(60),
            },
            at(10),
            jitter,
        );
        assert_eq!(state.active_pause(at(10)).map(Pause::until), Some(at(905)));
    }

    #[test]
    fn a_hit_after_a_pause_ran_out_starts_afresh() {
        let mut state = BudgetState::default();
        let jitter = Jitter::new(Duration::from_secs(5));
        state.hit(RateLimitHit::Primary { reset_at: at(100) }, at(0), jitter);
        state.hit(
            RateLimitHit::Secondary {
                retry_after: Duration::from_secs(60),
            },
            at(200),
            jitter,
        );
        let pause = state.active_pause(at(200)).expect("paused");
        assert_eq!(pause.until(), at(265));
        assert_eq!(pause.cause(), PauseCause::Secondary);
    }

    #[test]
    fn taking_the_tally_resets_it() {
        let mut state = BudgetState::default();
        state.spend.record_graphql(Feature::Feed, 3);
        let taken = state.take_spend();
        assert_eq!(taken.points(Feature::Feed), 3);
        assert!(state.spend.is_empty());
    }
}
