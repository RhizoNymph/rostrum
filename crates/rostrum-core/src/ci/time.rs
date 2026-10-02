//! How long a check has been running, or how long ago it finished.
//!
//! Every function takes `now` rather than reading the clock, so the labels
//! are tested against a fixed instant and a cell redrawn on a timer ticks
//! simply by being asked again.

use chrono::{DateTime, Duration, Utc};

use super::model::{CheckEntry, CheckStatus};

/// A span as a cell shows it: `45s`, `3m 12s`, `1h 04m`, `2d 03h`.
///
/// Two units at most — the grid is a glance, not a stopwatch — and the
/// smaller one zero-padded once the larger exists, so the label does not
/// change width every second. A negative span (clock skew between GitHub and
/// this machine) reads as `0s`.
pub fn format_duration(span: Duration) -> String {
    let secs = span.num_seconds().max(0);
    let (days, hours, minutes, seconds) = (
        secs / 86_400,
        secs % 86_400 / 3_600,
        secs % 3_600 / 60,
        secs % 60,
    );
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// How long ago something happened: `just now`, `14m ago`, `3h ago`,
/// `2d ago`. Coarser than [`format_duration`] on purpose: "finished 14m ago"
/// does not need seconds, and would flicker if it had them.
pub fn format_ago(span: Duration) -> String {
    let secs = span.num_seconds().max(0);
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3_600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3_600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// What a cell says about time, given `now`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Timing {
    /// Waiting to start: how long it has been queued, if GitHub said when.
    Queued(Option<Duration>),
    /// Running for this long. Ticks.
    Running(Duration),
    /// Finished this long ago, having taken `took` when both ends are known.
    Finished {
        ago: Duration,
        took: Option<Duration>,
    },
    /// Nothing to say: no timestamps at all.
    Unknown,
}

impl Timing {
    pub fn of(entry: &CheckEntry, now: DateTime<Utc>) -> Self {
        match (entry.status, entry.started_at, entry.completed_at) {
            (CheckStatus::Queued, started, _) => Self::Queued(started.map(|at| now - at)),
            (CheckStatus::InProgress, Some(started), _) => Self::Running(now - started),
            (CheckStatus::InProgress, None, _) => Self::Unknown,
            (_, started, Some(completed)) => Self::Finished {
                ago: now - completed,
                took: started
                    .filter(|started| *started < completed)
                    .map(|started| completed - started),
            },
            (_, Some(started), None) => Self::Finished {
                ago: now - started,
                took: None,
            },
            (_, None, None) => Self::Unknown,
        }
    }

    /// The cell's line: `3m 12s`, `queued 2m 00s`, `finished 14m ago`.
    pub fn label(&self) -> Option<String> {
        match self {
            Self::Queued(Some(waited)) => Some(format!("queued {}", format_duration(*waited))),
            Self::Queued(None) => Some("queued".into()),
            Self::Running(running) => Some(format_duration(*running)),
            Self::Finished { ago, .. } => Some(format!("finished {}", format_ago(*ago))),
            Self::Unknown => None,
        }
    }

    /// The hover text: how long it took, or has taken so far.
    pub fn duration_label(&self) -> Option<String> {
        match self {
            Self::Running(running) => Some(format!("running for {}", format_duration(*running))),
            Self::Finished {
                took: Some(took), ..
            } => Some(format!("took {}", format_duration(*took))),
            Self::Queued(Some(waited)) => Some(format!("queued for {}", format_duration(*waited))),
            _ => None,
        }
    }

    /// Whether the label changes from one second to the next, and so needs a
    /// ticking redraw.
    pub fn ticks(&self) -> bool {
        matches!(self, Self::Running(_) | Self::Queued(Some(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::model::{CheckKey, CheckSource};

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid")
    }

    /// The fixed clock every timing test reads.
    fn now() -> DateTime<Utc> {
        at(1_000_000)
    }

    fn entry(status: CheckStatus, started: Option<i64>, completed: Option<i64>) -> CheckEntry {
        CheckEntry {
            key: CheckKey::new(None, "ci"),
            status,
            started_at: started.map(at),
            completed_at: completed.map(at),
            details_url: None,
            source: CheckSource::Status,
        }
    }

    #[test]
    fn durations_use_two_units_with_the_smaller_padded() {
        for (secs, want) in [
            (0, "0s"),
            (45, "45s"),
            (60, "1m 00s"),
            (192, "3m 12s"),
            (3_599, "59m 59s"),
            (3_600, "1h 00m"),
            (3_840, "1h 04m"),
            (86_400 + 3 * 3_600 + 59, "1d 03h"),
            (-30, "0s"),
        ] {
            assert_eq!(format_duration(Duration::seconds(secs)), want, "{secs}");
        }
    }

    #[test]
    fn ago_is_coarse() {
        for (secs, want) in [
            (0, "just now"),
            (59, "just now"),
            (60, "1m ago"),
            (14 * 60 + 59, "14m ago"),
            (3 * 3_600, "3h ago"),
            (2 * 86_400 + 5, "2d ago"),
            (-10, "just now"),
        ] {
            assert_eq!(format_ago(Duration::seconds(secs)), want, "{secs}");
        }
    }

    #[test]
    fn a_running_check_shows_elapsed_time_and_ticks() {
        let timing = Timing::of(
            &entry(CheckStatus::InProgress, Some(1_000_000 - 192), None),
            now(),
        );
        assert_eq!(timing, Timing::Running(Duration::seconds(192)));
        assert_eq!(timing.label().as_deref(), Some("3m 12s"));
        assert_eq!(
            timing.duration_label().as_deref(),
            Some("running for 3m 12s")
        );
        assert!(timing.ticks());
    }

    #[test]
    fn a_finished_check_shows_when_and_how_long() {
        let timing = Timing::of(
            &entry(
                CheckStatus::Failure,
                Some(1_000_000 - 900 - 243),
                Some(1_000_000 - 900),
            ),
            now(),
        );
        assert_eq!(timing.label().as_deref(), Some("finished 15m ago"));
        assert_eq!(timing.duration_label().as_deref(), Some("took 4m 03s"));
        assert!(!timing.ticks());
    }

    #[test]
    fn a_queued_check_counts_its_wait_when_it_has_one() {
        let waiting = Timing::of(
            &entry(CheckStatus::Queued, Some(1_000_000 - 120), None),
            now(),
        );
        assert_eq!(waiting.label().as_deref(), Some("queued 2m 00s"));
        let unstarted = Timing::of(&entry(CheckStatus::Queued, None, None), now());
        assert_eq!(unstarted.label().as_deref(), Some("queued"));
        assert!(!unstarted.ticks());
    }

    /// A legacy status records one time: when its state was set. Finished,
    /// that is when it finished, and there is no duration to show.
    #[test]
    fn a_legacy_status_has_one_timestamp() {
        let done = Timing::of(
            &entry(CheckStatus::Success, Some(1_000_000 - 7_200), None),
            now(),
        );
        assert_eq!(done.label().as_deref(), Some("finished 2h ago"));
        assert_eq!(done.duration_label(), None);
        let pending = Timing::of(
            &entry(CheckStatus::InProgress, Some(1_000_000 - 5), None),
            now(),
        );
        assert_eq!(pending.label().as_deref(), Some("5s"));
    }

    #[test]
    fn no_timestamps_says_nothing() {
        let timing = Timing::of(&entry(CheckStatus::Skipped, None, None), now());
        assert_eq!(timing, Timing::Unknown);
        assert_eq!(timing.label(), None);
    }
}
