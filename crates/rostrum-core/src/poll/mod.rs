//! What a poll fetches: the pure planner behind the desktop store's poll
//! loop, shared so the Android core can adopt the same rules.
//!
//! A poll always refreshes the open pull requests (they carry the head SHAs
//! everything else is judged by). Issues ride their own, slower cadence
//! ([`issues_due`]). CI checks are planned after the pull requests land, and
//! only for repositories where something could have changed ([`ci_due`]).
//! The Closed tab's lists ride along only while that tab shows. An explicit
//! refresh ([`Trigger::Explicit`]) takes everything; a [`Pause`] takes
//! nothing. See `docs/features/poll_budget.md`.

mod ci;
mod issues;
mod plan;

use std::time::Duration;

pub use ci::{CiDue, CiReason, ci_due, plan_ci, running_repos};
pub use issues::issues_due;
pub use plan::{PollContext, PollDecision, PollPlan, plan_poll};

use crate::budget::Pause;

/// How often issues are refreshed unless configured otherwise. Issues change
/// far less often than pull requests and nothing on them is time-critical,
/// and they used to cost as much as the feed itself.
pub const DEFAULT_ISSUE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// How long a repository's checks are reused at most, even when nothing
/// suggests they changed.
///
/// A check another app adds to an unchanged head (a late-arriving status, a
/// scheduled re-run, a manual re-run in the web UI) changes nothing the
/// feed can see. Ten minutes bounds how stale that can get while costing a
/// fraction of polling every minute; anything the user starts from rostrum
/// is already queued locally and re-fetched by the running rule.
pub const CI_SAFETY_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// What started a poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The poll loop's timer: fetch what is due.
    Timer,
    /// The user asked (the Refresh action): fetch everything.
    Explicit,
}

/// The intervals the planner judges staleness by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    pub issues: Duration,
    pub ci_safety: Duration,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            issues: DEFAULT_ISSUE_INTERVAL,
            ci_safety: CI_SAFETY_INTERVAL,
        }
    }
}

/// Whether `pause` (if any) still holds at `now`.
fn paused(pause: Option<&Pause>, now: chrono::DateTime<chrono::Utc>) -> Option<Pause> {
    pause.filter(|pause| pause.is_active(now)).copied()
}

/// `true` once `at` is at least `age` before `now`.
fn older_than(
    at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
    age: Duration,
) -> bool {
    let age = chrono::Duration::from_std(age).unwrap_or(chrono::Duration::MAX);
    now.signed_duration_since(at) >= age
}
