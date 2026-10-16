//! What GitHub says is left of a budget.

use std::num::NonZeroU32;

use chrono::{DateTime, Utc};

/// Above this fraction of the limit left, the budget is healthy and polling
/// runs at its configured interval.
pub const LOW_FRACTION: f64 = 0.20;

/// Below this fraction left, the budget is critical.
pub const CRITICAL_FRACTION: f64 = 0.10;

/// Two observations whose reset times are this close belong to the same
/// window: `x-ratelimit-reset` is whole seconds, `rateLimit.resetAt` is a
/// timestamp, and the two can round apart.
const SAME_WINDOW_SLACK_SECS: i64 = 2;

/// Which of GitHub's separately metered budgets an observation is about.
/// Others GitHub reports (search, code scanning, …) are not tracked: nothing
/// here spends them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RateResource {
    /// GraphQL points: 5,000 an hour for a user token.
    Graphql,
    /// REST requests: 5,000 an hour for a user token.
    Core,
}

impl RateResource {
    /// From the `x-ratelimit-resource` header.
    pub fn from_header(value: &str) -> Option<Self> {
        match value.trim() {
            "graphql" => Some(Self::Graphql),
            "core" => Some(Self::Core),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Graphql => "GraphQL",
            Self::Core => "REST",
        }
    }
}

/// One budget as GitHub last reported it. The limit is never zero and the
/// remainder never exceeds it — [`RateBudget::new`] refuses or clamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateBudget {
    limit: NonZeroU32,
    remaining: u32,
    reset_at: DateTime<Utc>,
}

/// How much of a budget is left, coarsely: what colours the readout and
/// stretches the poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetLevel {
    /// Nothing left until the reset.
    Exhausted,
    /// Under [`CRITICAL_FRACTION`].
    Critical,
    /// Under [`LOW_FRACTION`].
    Low,
    Healthy,
}

impl RateBudget {
    /// `None` for a zero limit, which no real budget has. A remainder above
    /// the limit is clamped to it.
    pub fn new(limit: u32, remaining: u32, reset_at: DateTime<Utc>) -> Option<Self> {
        let limit = NonZeroU32::new(limit)?;
        Some(Self {
            limit,
            remaining: remaining.min(limit.get()),
            reset_at,
        })
    }

    pub fn limit(&self) -> u32 {
        self.limit.get()
    }

    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    pub fn reset_at(&self) -> DateTime<Utc> {
        self.reset_at
    }

    /// Whether the window this observation belongs to has ended by `now`, so
    /// the budget is full again whatever it said.
    pub fn has_reset(&self, now: DateTime<Utc>) -> bool {
        now >= self.reset_at
    }

    /// What is left at `now`: the reported remainder, or the whole limit once
    /// the window has reset.
    pub fn remaining_at(&self, now: DateTime<Utc>) -> u32 {
        if self.has_reset(now) {
            self.limit()
        } else {
            self.remaining
        }
    }

    /// Fraction of the limit left at `now`, in `0.0..=1.0`.
    pub fn fraction_at(&self, now: DateTime<Utc>) -> f64 {
        f64::from(self.remaining_at(now)) / f64::from(self.limit())
    }

    pub fn level_at(&self, now: DateTime<Utc>) -> BudgetLevel {
        let left = self.remaining_at(now);
        if left == 0 {
            return BudgetLevel::Exhausted;
        }
        let fraction = self.fraction_at(now);
        if fraction < CRITICAL_FRACTION {
            BudgetLevel::Critical
        } else if fraction < LOW_FRACTION {
            BudgetLevel::Low
        } else {
            BudgetLevel::Healthy
        }
    }

    /// Fold in an observation from another response.
    ///
    /// Responses land out of order, and within one window the remainder only
    /// ever falls, so the same window keeps the lower remainder. A later
    /// window replaces this one; an observation from an earlier window — a
    /// slow response from before the reset — is ignored.
    pub fn merged(self, other: RateBudget) -> RateBudget {
        let gap = other
            .reset_at
            .signed_duration_since(self.reset_at)
            .num_seconds();
        if gap > SAME_WINDOW_SLACK_SECS {
            other
        } else if gap < -SAME_WINDOW_SLACK_SECS {
            self
        } else if other.remaining < self.remaining || other.limit != self.limit {
            other
        } else {
            self
        }
    }
}

/// The tracked budgets, each as last observed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budgets {
    pub graphql: Option<RateBudget>,
    pub core: Option<RateBudget>,
}

impl Budgets {
    pub fn get(&self, resource: RateResource) -> Option<&RateBudget> {
        match resource {
            RateResource::Graphql => self.graphql.as_ref(),
            RateResource::Core => self.core.as_ref(),
        }
    }

    pub fn observe(&mut self, resource: RateResource, budget: RateBudget) {
        let slot = match resource {
            RateResource::Graphql => &mut self.graphql,
            RateResource::Core => &mut self.core,
        };
        *slot = Some(match slot.take() {
            Some(held) => held.merged(budget),
            None => budget,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    fn budget(remaining: u32, reset: i64) -> RateBudget {
        RateBudget::new(5000, remaining, at(reset)).expect("valid budget")
    }

    #[test]
    fn a_zero_limit_is_not_a_budget() {
        assert!(RateBudget::new(0, 0, at(0)).is_none());
    }

    #[test]
    fn a_remainder_above_the_limit_is_clamped() {
        assert_eq!(
            RateBudget::new(100, 250, at(0)).map(|b| b.remaining()),
            Some(100)
        );
    }

    #[test]
    fn resources_come_from_the_header() {
        assert_eq!(
            RateResource::from_header("graphql"),
            Some(RateResource::Graphql)
        );
        assert_eq!(
            RateResource::from_header(" core "),
            Some(RateResource::Core)
        );
        assert_eq!(RateResource::from_header("search"), None);
    }

    #[test]
    fn levels_follow_the_thresholds() {
        let now = at(0);
        assert_eq!(budget(5000, 60).level_at(now), BudgetLevel::Healthy);
        assert_eq!(
            budget(1000, 60).level_at(now),
            BudgetLevel::Healthy,
            "exactly 20%"
        );
        assert_eq!(budget(999, 60).level_at(now), BudgetLevel::Low);
        assert_eq!(
            budget(500, 60).level_at(now),
            BudgetLevel::Low,
            "exactly 10%"
        );
        assert_eq!(budget(499, 60).level_at(now), BudgetLevel::Critical);
        assert_eq!(budget(1, 60).level_at(now), BudgetLevel::Critical);
        assert_eq!(budget(0, 60).level_at(now), BudgetLevel::Exhausted);
    }

    #[test]
    fn a_window_that_has_reset_is_full_again() {
        let spent = budget(0, 60);
        assert_eq!(spent.remaining_at(at(59)), 0);
        assert_eq!(spent.remaining_at(at(60)), 5000);
        assert_eq!(spent.level_at(at(61)), BudgetLevel::Healthy);
    }

    #[test]
    fn the_same_window_keeps_the_lower_remainder() {
        let first = budget(4000, 600);
        assert_eq!(
            first.merged(budget(4100, 600)).remaining(),
            4000,
            "late response"
        );
        assert_eq!(
            first.merged(budget(3900, 601)).remaining(),
            3900,
            "rounding slack"
        );
    }

    #[test]
    fn a_later_window_replaces_and_an_earlier_one_is_ignored() {
        let current = budget(100, 600);
        assert_eq!(current.merged(budget(4990, 4200)).remaining(), 4990);
        assert_eq!(current.merged(budget(10, 0)).remaining(), 100);
    }

    #[test]
    fn budgets_are_kept_per_resource() {
        let mut budgets = Budgets::default();
        budgets.observe(RateResource::Graphql, budget(4000, 600));
        budgets.observe(RateResource::Core, budget(4900, 900));
        budgets.observe(RateResource::Graphql, budget(3990, 600));
        assert_eq!(
            budgets
                .get(RateResource::Graphql)
                .map(RateBudget::remaining),
            Some(3990)
        );
        assert_eq!(
            budgets.get(RateResource::Core).map(RateBudget::remaining),
            Some(4900)
        );
    }
}
