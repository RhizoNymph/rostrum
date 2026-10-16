//! Stretching the poll interval as the budget runs down.
//!
//! The policy, in order:
//!
//! 1. Unknown budget, a window that has already reset, or more than
//!    [`LOW_FRACTION`](super::LOW_FRACTION) left: the configured interval.
//! 2. Otherwise hold back a reserve ([`RESERVE_FRACTION`] of the limit) for
//!    what the user does by hand — opening a pull request, merging, `gh` in a
//!    terminal — and, if what is left above it cannot pay for one more poll,
//!    wait for the reset.
//! 3. Otherwise wait the longer of a floor — twice the interval when low,
//!    four times when critical — and the pace that makes the usable
//!    remainder last until the reset at the current cost per poll.
//! 4. Never wait past the reset (the budget is full again then), unless the
//!    configured interval itself is longer.
//!
//! Pacing to the reset, rather than multipliers alone, is what makes the
//! budget last when something else (`gh`, the phone) is spending it too: the
//! remainder it divides is what GitHub reports, whoever spent the rest.

use std::time::Duration;

use chrono::{DateTime, Utc};

use super::rate::{BudgetLevel, RateBudget};

/// Fraction of the limit polling leaves alone for interactive use.
pub const RESERVE_FRACTION: f64 = 0.02;

/// Points held back from polling out of a budget of `limit`.
pub fn reserve(limit: u32) -> u32 {
    (f64::from(limit) * RESERVE_FRACTION).ceil() as u32
}

/// Why the interval is what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaceReason {
    /// The configured interval.
    Normal,
    /// Stretched because the budget is `level`.
    Stretched { level: BudgetLevel },
    /// Nothing usable left before the reset: wait for it.
    UntilReset,
}

/// The wait before the next poll, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
    pub delay: Duration,
    pub reason: PaceReason,
}

/// The wait before the next poll, given the configured interval `base`, the
/// latest GraphQL budget, and roughly what one poll costs in points.
pub fn pace(
    base: Duration,
    budget: Option<&RateBudget>,
    cost_per_poll: u32,
    now: DateTime<Utc>,
) -> Pace {
    let normal = Pace {
        delay: base,
        reason: PaceReason::Normal,
    };
    let Some(budget) = budget else {
        return normal;
    };
    if budget.has_reset(now) {
        return normal;
    }
    let level = budget.level_at(now);
    if level == BudgetLevel::Healthy {
        return normal;
    }

    let to_reset = budget
        .reset_at()
        .signed_duration_since(now)
        .to_std()
        .unwrap_or(Duration::ZERO);
    let ceiling = to_reset.max(base);
    let cost = cost_per_poll.max(1);
    let usable = budget.remaining().saturating_sub(reserve(budget.limit()));
    if usable < cost {
        return Pace {
            delay: ceiling,
            reason: PaceReason::UntilReset,
        };
    }

    let floor = base
        * match level {
            BudgetLevel::Low => 2,
            _ => 4,
        };
    let polls_left = usable / cost;
    let paced = to_reset / polls_left;
    let delay = floor.max(paced).min(ceiling);
    Pace {
        delay,
        reason: if delay > base {
            PaceReason::Stretched { level }
        } else {
            PaceReason::Normal
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Duration = Duration::from_secs(60);

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    fn budget(remaining: u32, reset_in: i64) -> RateBudget {
        RateBudget::new(5000, remaining, at(reset_in)).expect("valid budget")
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_reserve_is_two_percent_rounded_up() {
        assert_eq!(reserve(5000), 100);
        assert_eq!(reserve(60), 2);
    }

    #[test]
    fn unknown_or_healthy_budgets_poll_normally() {
        let now = at(0);
        assert_eq!(pace(BASE, None, 10, now).delay, BASE);
        let healthy = pace(BASE, Some(&budget(4000, 3000)), 10, now);
        assert_eq!(
            healthy,
            Pace {
                delay: BASE,
                reason: PaceReason::Normal
            }
        );
        let at_threshold = pace(BASE, Some(&budget(1000, 3000)), 10, now);
        assert_eq!(at_threshold.delay, BASE);
    }

    #[test]
    fn a_window_that_has_reset_polls_normally() {
        assert_eq!(pace(BASE, Some(&budget(0, -5)), 10, at(0)).delay, BASE);
    }

    #[test]
    fn low_doubles_the_interval() {
        // 900 left, 800 usable, 80 polls over 3000 s = 37.5 s each: the
        // floor of 2 × 60 s wins.
        let paced = pace(BASE, Some(&budget(900, 3000)), 10, at(0));
        assert_eq!(paced.delay, secs(120));
        assert_eq!(
            paced.reason,
            PaceReason::Stretched {
                level: BudgetLevel::Low
            }
        );
    }

    #[test]
    fn critical_quadruples_the_interval() {
        let paced = pace(BASE, Some(&budget(400, 3000)), 1, at(0));
        assert_eq!(paced.delay, secs(240));
        assert_eq!(
            paced.reason,
            PaceReason::Stretched {
                level: BudgetLevel::Critical
            }
        );
    }

    #[test]
    fn pacing_to_the_reset_wins_when_it_is_longer() {
        // 300 left, 200 usable, 20 a poll: 10 polls over 3000 s.
        let paced = pace(BASE, Some(&budget(300, 3000)), 20, at(0));
        assert_eq!(paced.delay, secs(300));
    }

    #[test]
    fn never_waits_past_the_reset() {
        let paced = pace(BASE, Some(&budget(400, 100)), 1, at(0));
        assert_eq!(paced.delay, secs(100));
    }

    #[test]
    fn a_reset_sooner_than_the_interval_still_waits_the_interval() {
        let paced = pace(BASE, Some(&budget(400, 10)), 1, at(0));
        assert_eq!(paced.delay, BASE);
        assert_eq!(paced.reason, PaceReason::Normal);
    }

    #[test]
    fn nothing_usable_waits_for_the_reset() {
        let paced = pace(BASE, Some(&budget(105, 1800)), 10, at(0));
        assert_eq!(
            paced,
            Pace {
                delay: secs(1800),
                reason: PaceReason::UntilReset
            }
        );
        let spent = pace(BASE, Some(&budget(0, 1800)), 10, at(0));
        assert_eq!(spent.reason, PaceReason::UntilReset);
    }

    #[test]
    fn a_zero_cost_estimate_counts_as_one_point() {
        let paced = pace(BASE, Some(&budget(110, 3000)), 0, at(0));
        // 10 usable, 10 polls over 3000 s.
        assert_eq!(paced.delay, secs(300));
    }
}
