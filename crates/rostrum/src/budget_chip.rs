//! The status bar's rate-limit readout: GraphQL points left of the limit
//! and when they reset, coloured by how low they are, or "paused until" while
//! GitHub refuses. The wording is pure, so it is tested without a window.

use chrono::{DateTime, Local, Utc};
use rostrum_core::budget::{
    BudgetLevel, BudgetState, LOW_FRACTION, Pace, PaceReason, PauseCause, RateBudget,
};

/// How loudly the chip reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Quiet,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetChip {
    pub label: String,
    pub tone: Tone,
    pub tooltip: String,
}

/// `HH:MM` in the local time zone.
pub fn local_clock(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%H:%M").to_string()
}

const SHARED: &str = "GitHub's API budget for this token, shared with the gh CLI and a paired \
                      phone — what they spend counts here too.";

fn line(
    name: &str,
    unit: &str,
    budget: &RateBudget,
    now: DateTime<Utc>,
    clock: &impl Fn(DateTime<Utc>) -> String,
) -> String {
    if budget.has_reset(now) {
        format!("{name}: full again ({} {unit}).", budget.limit())
    } else {
        format!(
            "{name}: {} of {} {unit} left, resets at {}.",
            budget.remaining(),
            budget.limit(),
            clock(budget.reset_at())
        )
    }
}

/// The chip for `state` at `now`, or `None` before any response has said
/// anything about the budget. `clock` formats a time for display.
pub fn budget_chip(
    state: &BudgetState,
    pace: Option<Pace>,
    now: DateTime<Utc>,
    clock: impl Fn(DateTime<Utc>) -> String,
) -> Option<BudgetChip> {
    let mut tooltip = vec![SHARED.to_string()];
    if let Some(graphql) = &state.budgets.graphql {
        tooltip.push(line("GraphQL", "points", graphql, now, &clock));
    }
    if let Some(core) = &state.budgets.core {
        tooltip.push(line("REST", "requests", core, now, &clock));
    }
    let slowing = format!(
        "Polling slows below {:.0}% left and stops while GitHub refuses.",
        LOW_FRACTION * 100.0
    );
    match pace.map(|pace| (pace.reason, pace.delay)) {
        Some((PaceReason::Stretched { .. } | PaceReason::UntilReset, delay)) => {
            tooltip.push(format!(
                "{slowing} Polling every {} s now.",
                delay.as_secs()
            ));
        }
        _ => tooltip.push(slowing),
    }

    if let Some(pause) = state.active_pause(now) {
        let why = match pause.cause() {
            PauseCause::Primary => "GitHub says the hourly budget is spent",
            PauseCause::Secondary => "GitHub asked for a break (secondary rate limit)",
        };
        tooltip.insert(
            1,
            format!("{why}; every poll waits until {}.", clock(pause.until())),
        );
        return Some(BudgetChip {
            label: format!("paused until {}", clock(pause.until())),
            tone: Tone::Danger,
            tooltip: tooltip.join("\n"),
        });
    }

    let graphql = state.budgets.graphql.as_ref()?;
    let tone = match graphql.level_at(now) {
        BudgetLevel::Healthy => Tone::Quiet,
        BudgetLevel::Low => Tone::Warning,
        BudgetLevel::Critical | BudgetLevel::Exhausted => Tone::Danger,
    };
    let label = if graphql.has_reset(now) {
        format!("API {0}/{0}", graphql.limit())
    } else {
        format!(
            "API {}/{} · resets {}",
            graphql.remaining(),
            graphql.limit(),
            clock(graphql.reset_at())
        )
    };
    Some(BudgetChip {
        label,
        tone,
        tooltip: tooltip.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rostrum_core::budget::{Jitter, RateLimitHit, RateResource};

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    /// Seconds after the fixed instant, so the tests do not depend on the
    /// machine's time zone.
    fn clock(time: DateTime<Utc>) -> String {
        format!("t+{}", time.timestamp() - 1_800_000_000)
    }

    fn with_graphql(remaining: u32) -> BudgetState {
        let mut state = BudgetState::default();
        state.observe(
            RateResource::Graphql,
            RateBudget::new(5000, remaining, at(1800)).expect("budget"),
        );
        state
    }

    #[test]
    fn nothing_known_shows_nothing() {
        assert!(budget_chip(&BudgetState::default(), None, at(0), clock).is_none());
    }

    #[test]
    fn a_healthy_budget_reads_quietly_with_its_reset() {
        let chip = budget_chip(&with_graphql(4123), None, at(0), clock).expect("chip");
        assert_eq!(chip.label, "API 4123/5000 · resets t+1800");
        assert_eq!(chip.tone, Tone::Quiet);
        assert!(chip.tooltip.contains("gh CLI"));
        assert!(chip.tooltip.contains("paired phone"));
        assert!(chip.tooltip.contains("4123 of 5000 points left"));
    }

    #[test]
    fn the_tone_follows_the_level() {
        let tone =
            |remaining| budget_chip(&with_graphql(remaining), None, at(0), clock).map(|c| c.tone);
        assert_eq!(tone(999), Some(Tone::Warning));
        assert_eq!(tone(499), Some(Tone::Danger));
        assert_eq!(tone(0), Some(Tone::Danger));
    }

    #[test]
    fn a_reset_window_reads_full() {
        let chip = budget_chip(&with_graphql(10), None, at(1800), clock).expect("chip");
        assert_eq!(chip.label, "API 5000/5000");
        assert_eq!(chip.tone, Tone::Quiet);
    }

    #[test]
    fn a_pause_reads_paused_until() {
        let mut state = with_graphql(0);
        state.hit(
            RateLimitHit::Primary { reset_at: at(1800) },
            at(0),
            Jitter::new(Duration::from_secs(10)),
        );
        let chip = budget_chip(&state, None, at(5), clock).expect("chip");
        assert_eq!(chip.label, "paused until t+1810");
        assert_eq!(chip.tone, Tone::Danger);
        assert!(chip.tooltip.contains("hourly budget is spent"));
    }

    #[test]
    fn a_secondary_pause_says_so_even_before_any_budget_is_known() {
        let mut state = BudgetState::default();
        state.hit(
            RateLimitHit::Secondary {
                retry_after: Duration::from_secs(60),
            },
            at(0),
            Jitter::new(Duration::from_secs(5)),
        );
        let chip = budget_chip(&state, None, at(1), clock).expect("chip");
        assert_eq!(chip.label, "paused until t+65");
        assert!(chip.tooltip.contains("secondary rate limit"));
    }

    #[test]
    fn a_stretched_pace_is_explained() {
        let pace = Pace {
            delay: Duration::from_secs(240),
            reason: PaceReason::Stretched {
                level: BudgetLevel::Critical,
            },
        };
        let chip = budget_chip(&with_graphql(400), Some(pace), at(0), clock).expect("chip");
        assert!(chip.tooltip.contains("every 240 s"));
    }
}
