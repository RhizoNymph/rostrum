//! The periodic log line of what each part of the app spent.

use std::time::{Duration, Instant};

use rostrum_core::budget::{BudgetState, Feature, SpendTally};

/// How often the points spent per feature are logged, as on the desktop.
pub(crate) const SPEND_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// When the spend was last logged. The phone has no timer of its own for
/// this: every refresh asks whether a report is due.
#[derive(Debug)]
pub(crate) struct SpendLog {
    last: Instant,
}

impl SpendLog {
    pub(crate) fn new(now: Instant) -> Self {
        Self { last: now }
    }

    /// At least [`SPEND_LOG_INTERVAL`] since the last report (or the start).
    pub(crate) fn due(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last) >= SPEND_LOG_INTERVAL
    }

    pub(crate) fn logged(&mut self, now: Instant) {
        self.last = now;
    }
}

/// One structured line: points per feature, request counts, and what is
/// left. Nothing when nothing was spent.
pub(crate) fn log(spent: &SpendTally, budget: &BudgetState) {
    if spent.is_empty() {
        return;
    }
    let points = |feature| spent.points(feature);
    tracing::info!(
        target: "rostrum::budget",
        feed = points(Feature::Feed),
        issues = points(Feature::Issues),
        ci = points(Feature::Ci),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_due_every_five_minutes() {
        let start = Instant::now();
        let mut log = SpendLog::new(start);
        assert!(!log.due(start));
        assert!(!log.due(start + Duration::from_secs(299)));
        assert!(log.due(start + SPEND_LOG_INTERVAL));
        log.logged(start + SPEND_LOG_INTERVAL);
        assert!(!log.due(start + Duration::from_secs(400)));
        assert!(log.due(start + 2 * SPEND_LOG_INTERVAL));
    }
}
