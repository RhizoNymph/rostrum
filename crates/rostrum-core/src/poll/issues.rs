//! When a repository's open issues are due.

use std::time::Duration;

use chrono::{DateTime, Utc};

use super::{Trigger, older_than};
use crate::state::LoadState;

/// Whether a repository's issues should be fetched now.
///
/// Never fetched, or the last fetch failed: due. In flight: not due (the
/// fetch under way will answer). Loaded: due once older than `cadence`, or
/// at once for an explicit refresh.
pub fn issues_due(
    load: &LoadState,
    now: DateTime<Utc>,
    cadence: Duration,
    trigger: Trigger,
) -> bool {
    match load {
        LoadState::Loading => false,
        LoadState::Idle | LoadState::Failed { .. } => true,
        LoadState::Loaded { at } => trigger == Trigger::Explicit || older_than(*at, now, cadence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CADENCE: Duration = Duration::from_secs(300);

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    #[test]
    fn never_fetched_or_failed_is_due() {
        assert!(issues_due(&LoadState::Idle, at(0), CADENCE, Trigger::Timer));
        let failed = LoadState::Failed {
            message: "boom".into(),
            at: at(-1),
        };
        assert!(issues_due(&failed, at(0), CADENCE, Trigger::Timer));
    }

    #[test]
    fn in_flight_is_not_due_even_when_asked() {
        assert!(!issues_due(
            &LoadState::Loading,
            at(0),
            CADENCE,
            Trigger::Timer
        ));
        assert!(!issues_due(
            &LoadState::Loading,
            at(0),
            CADENCE,
            Trigger::Explicit
        ));
    }

    #[test]
    fn a_fresh_list_waits_for_its_cadence() {
        let loaded = LoadState::Loaded { at: at(0) };
        assert!(!issues_due(&loaded, at(60), CADENCE, Trigger::Timer));
        assert!(!issues_due(&loaded, at(299), CADENCE, Trigger::Timer));
        assert!(issues_due(&loaded, at(300), CADENCE, Trigger::Timer));
    }

    #[test]
    fn an_explicit_refresh_takes_a_fresh_list_too() {
        let loaded = LoadState::Loaded { at: at(0) };
        assert!(issues_due(&loaded, at(1), CADENCE, Trigger::Explicit));
    }
}
