//! What one CI check is: its identity, status, timing and where it came from.

use std::{collections::BTreeMap, fmt};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::PrNumber;

/// A check's status, as the grid shows it.
///
/// GitHub splits a check run into `status` (queued, in progress, completed)
/// and, once completed, `conclusion`. The grid only ever needs one word, so
/// the pair is collapsed here — see [`CheckStatus::from_check_run`] — and a
/// legacy commit status's single `state` maps onto the same scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Queued,
    InProgress,
    Success,
    Failure,
    Cancelled,
    Skipped,
    Neutral,
    TimedOut,
    ActionRequired,
}

impl CheckStatus {
    /// A GraphQL `CheckRun`'s `status` and `conclusion`.
    ///
    /// Anything not yet completed is queued or in progress. A completed run's
    /// conclusion picks the rest; `STARTUP_FAILURE` is a failure, `STALE` (a
    /// run GitHub gave up on) reads as cancelled, and a conclusion added later
    /// reads as neutral rather than failing the decode.
    pub fn from_check_run(status: &str, conclusion: Option<&str>) -> Self {
        match status {
            "IN_PROGRESS" => return Self::InProgress,
            "COMPLETED" => {}
            // QUEUED, WAITING, PENDING, REQUESTED, and anything newer.
            _ => return Self::Queued,
        }
        match conclusion {
            Some("SUCCESS") => Self::Success,
            Some("FAILURE" | "STARTUP_FAILURE") => Self::Failure,
            Some("CANCELLED" | "STALE") => Self::Cancelled,
            Some("SKIPPED") => Self::Skipped,
            Some("TIMED_OUT") => Self::TimedOut,
            Some("ACTION_REQUIRED") => Self::ActionRequired,
            _ => Self::Neutral,
        }
    }

    /// A legacy `StatusContext`'s `state`. Pending is shown as running: the
    /// status API has no separate queued state.
    pub fn from_status_context(state: &str) -> Self {
        match state {
            "PENDING" | "EXPECTED" => Self::InProgress,
            "SUCCESS" => Self::Success,
            "FAILURE" | "ERROR" => Self::Failure,
            _ => Self::Neutral,
        }
    }

    /// Queued or in progress: not finished, and worth polling for.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Queued | Self::InProgress)
    }

    /// A result that needs attention.
    pub fn is_failing(self) -> bool {
        matches!(self, Self::Failure | Self::TimedOut | Self::ActionRequired)
    }

    pub fn is_finished(self) -> bool {
        !self.is_running()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::InProgress => "in progress",
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
            Self::Neutral => "neutral",
            Self::TimedOut => "timed out",
            Self::ActionRequired => "action required",
        }
    }
}

/// A grid column: what a check is called.
///
/// Actions jobs are keyed by workflow *and* job name, since two workflows
/// routinely both have a `build` job. Third-party check runs and legacy
/// statuses have no workflow. Ordered by workflow then name — `None` first —
/// which groups a workflow's jobs side by side.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CheckKey {
    pub workflow: Option<String>,
    pub name: String,
}

impl CheckKey {
    pub fn new(workflow: Option<String>, name: impl Into<String>) -> Self {
        Self {
            workflow,
            name: name.into(),
        }
    }
}

impl fmt::Display for CheckKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.workflow {
            Some(workflow) => write!(f, "{workflow} / {}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// Where a check came from, which decides what can be done with it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CheckSource {
    /// A GitHub Actions job. Its check-run id is the job id the logs and
    /// re-run endpoints take.
    Actions {
        job_id: u64,
        run_id: u64,
        run_attempt: u32,
        suite_id: u64,
    },
    /// A check run posted by another GitHub App.
    App {
        check_run_id: u64,
        suite_id: Option<u64>,
        app: String,
    },
    /// A legacy commit status: a state and a link, nothing more.
    Status,
}

/// One check on a pull request's head commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckEntry {
    pub key: CheckKey,
    pub status: CheckStatus,
    /// When it started; for a legacy status, when the state was set.
    pub started_at: Option<DateTime<Utc>>,
    /// When it finished; `None` while running. A legacy status records only
    /// one time, so a finished one has this equal to `started_at`.
    pub completed_at: Option<DateTime<Utc>>,
    /// Where the check's own page is: the job page, the app's page, or the
    /// status's target.
    pub details_url: Option<String>,
    pub source: CheckSource,
}

impl CheckEntry {
    /// The display name of what produced it: the app, or "GitHub Actions".
    pub fn producer(&self) -> &str {
        match &self.source {
            CheckSource::Actions { .. } => "GitHub Actions",
            CheckSource::App { app, .. } => app,
            CheckSource::Status => "the status provider",
        }
    }

    pub fn run_id(&self) -> Option<u64> {
        match self.source {
            CheckSource::Actions { run_id, .. } => Some(run_id),
            _ => None,
        }
    }

    /// Ordering for "which of two same-named checks is the latest": a later
    /// run attempt, then a later start (a queued check, not yet started, is
    /// the newest of all), then the larger id.
    fn recency(&self) -> (u32, Option<DateTime<Utc>>, u64) {
        let (attempt, id) = match &self.source {
            CheckSource::Actions {
                job_id,
                run_attempt,
                ..
            } => (*run_attempt, *job_id),
            CheckSource::App { check_run_id, .. } => (0, *check_run_id),
            CheckSource::Status => (0, 0),
        };
        let started = match self.started_at {
            None => Some(DateTime::<Utc>::MAX_UTC),
            some => some,
        };
        (attempt, started, id)
    }
}

/// The checks on one pull request's head commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrChecks {
    pub number: PrNumber,
    /// The commit the checks ran on.
    pub head_sha: String,
    pub entries: Vec<CheckEntry>,
    /// GitHub reported more contexts than were fetched. The grid says so
    /// rather than implying the missing ones never ran.
    pub truncated: bool,
}

impl PrChecks {
    /// One entry per column: when a commit carries several runs of the same
    /// check — a re-run, or the same job triggered by two events — the latest
    /// one is what the cell shows.
    pub fn latest(&self) -> BTreeMap<&CheckKey, &CheckEntry> {
        let mut latest: BTreeMap<&CheckKey, &CheckEntry> = BTreeMap::new();
        for entry in &self.entries {
            match latest.get(&entry.key) {
                Some(held) if held.recency() >= entry.recency() => {}
                _ => {
                    latest.insert(&entry.key, entry);
                }
            }
        }
        latest
    }

    /// The row's one-word summary, over the latest entry of each check.
    pub fn rollup(&self) -> Rollup {
        Rollup::of(self.latest().values().map(|entry| entry.status))
    }
}

/// A pull request's checks in one word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rollup {
    pub failing: usize,
    pub running: usize,
    pub passing: usize,
    /// Skipped, cancelled and neutral: finished, neither green nor red.
    pub other: usize,
}

/// The verdict of a [`Rollup`]: failure outranks running outranks success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollupState {
    Failing,
    Running,
    Passing,
    /// Only skipped/cancelled/neutral checks.
    Settled,
    /// No checks at all.
    Empty,
}

impl Rollup {
    pub fn of(statuses: impl IntoIterator<Item = CheckStatus>) -> Self {
        let mut rollup = Self::default();
        for status in statuses {
            match status {
                s if s.is_failing() => rollup.failing += 1,
                s if s.is_running() => rollup.running += 1,
                CheckStatus::Success => rollup.passing += 1,
                _ => rollup.other += 1,
            }
        }
        rollup
    }

    pub fn state(&self) -> RollupState {
        if self.failing > 0 {
            RollupState::Failing
        } else if self.running > 0 {
            RollupState::Running
        } else if self.passing > 0 {
            RollupState::Passing
        } else if self.other > 0 {
            RollupState::Settled
        } else {
            RollupState::Empty
        }
    }

    /// "2 failing · 3 running · 14 passed", zero parts omitted.
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            (self.failing, "failing"),
            (self.running, "running"),
            (self.passing, "passed"),
            (self.other, "other"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, word)| format!("{count} {word}"))
        .collect();
        if parts.is_empty() {
            "no checks".into()
        } else {
            parts.join(" · ")
        }
    }
}

/// What a non-Actions check run reports about itself, for its detail view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutput {
    pub title: Option<String>,
    /// Markdown.
    pub summary: Option<String>,
    /// Markdown; the longer body some apps fill.
    pub text: Option<String>,
    pub annotations: Vec<Annotation>,
}

/// How serious an annotation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationLevel {
    Notice,
    Warning,
    Failure,
}

/// A note a check run attached to a line of a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub level: AnnotationLevel,
    pub title: Option<String>,
    pub message: String,
}

impl Annotation {
    /// `src/lib.rs:12` or `src/lib.rs:12-14`.
    pub fn location(&self) -> String {
        if self.end_line > self.start_line {
            format!("{}:{}-{}", self.path, self.start_line, self.end_line)
        } else {
            format!("{}:{}", self.path, self.start_line)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid")
    }

    fn job(
        name: &str,
        status: CheckStatus,
        job_id: u64,
        attempt: u32,
        started: Option<i64>,
    ) -> CheckEntry {
        CheckEntry {
            key: CheckKey::new(Some("CI".into()), name),
            status,
            started_at: started.map(at),
            completed_at: None,
            details_url: None,
            source: CheckSource::Actions {
                job_id,
                run_id: 900 + u64::from(attempt),
                run_attempt: attempt,
                suite_id: 1,
            },
        }
    }

    #[test]
    fn check_run_status_and_conclusion_collapse_onto_one_word() {
        use CheckStatus as S;
        for (status, conclusion, want) in [
            ("QUEUED", None, S::Queued),
            ("WAITING", None, S::Queued),
            ("PENDING", None, S::Queued),
            ("REQUESTED", None, S::Queued),
            ("IN_PROGRESS", None, S::InProgress),
            ("COMPLETED", Some("SUCCESS"), S::Success),
            ("COMPLETED", Some("FAILURE"), S::Failure),
            ("COMPLETED", Some("STARTUP_FAILURE"), S::Failure),
            ("COMPLETED", Some("CANCELLED"), S::Cancelled),
            ("COMPLETED", Some("STALE"), S::Cancelled),
            ("COMPLETED", Some("SKIPPED"), S::Skipped),
            ("COMPLETED", Some("NEUTRAL"), S::Neutral),
            ("COMPLETED", Some("TIMED_OUT"), S::TimedOut),
            ("COMPLETED", Some("ACTION_REQUIRED"), S::ActionRequired),
            ("COMPLETED", Some("SOMETHING_NEW"), S::Neutral),
            ("COMPLETED", None, S::Neutral),
            ("SOMETHING_NEW", None, S::Queued),
        ] {
            assert_eq!(
                CheckStatus::from_check_run(status, conclusion),
                want,
                "{status} {conclusion:?}"
            );
        }
    }

    #[test]
    fn legacy_states_map_onto_the_same_scale() {
        assert_eq!(
            CheckStatus::from_status_context("PENDING"),
            CheckStatus::InProgress
        );
        assert_eq!(
            CheckStatus::from_status_context("EXPECTED"),
            CheckStatus::InProgress
        );
        assert_eq!(
            CheckStatus::from_status_context("SUCCESS"),
            CheckStatus::Success
        );
        assert_eq!(
            CheckStatus::from_status_context("FAILURE"),
            CheckStatus::Failure
        );
        assert_eq!(
            CheckStatus::from_status_context("ERROR"),
            CheckStatus::Failure
        );
    }

    #[test]
    fn keys_order_by_workflow_then_name_and_display_both() {
        let mut keys = [
            CheckKey::new(Some("Tests".into()), "unit"),
            CheckKey::new(None, "license/cla"),
            CheckKey::new(Some("Lint".into()), "clippy"),
            CheckKey::new(Some("Tests".into()), "e2e"),
        ];
        keys.sort();
        let shown: Vec<String> = keys.iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                "license/cla",
                "Lint / clippy",
                "Tests / e2e",
                "Tests / unit"
            ]
        );
    }

    /// A re-run makes a second check run of the same name; the later attempt
    /// is what the cell shows, however the two arrive.
    #[test]
    fn the_latest_attempt_wins_a_column() {
        let checks = PrChecks {
            number: PrNumber(1),
            head_sha: "abc".into(),
            entries: vec![
                job("build", CheckStatus::InProgress, 20, 2, Some(200)),
                job("build", CheckStatus::Failure, 10, 1, Some(100)),
                job("test", CheckStatus::Success, 30, 1, Some(100)),
                job("test", CheckStatus::Queued, 40, 1, None),
            ],
            truncated: false,
        };
        let latest = checks.latest();
        assert_eq!(latest.len(), 2);
        let build = latest[&CheckKey::new(Some("CI".into()), "build")];
        assert_eq!(build.status, CheckStatus::InProgress);
        // Same attempt: the one not yet started is the newest.
        let test = latest[&CheckKey::new(Some("CI".into()), "test")];
        assert_eq!(test.status, CheckStatus::Queued);
        assert_eq!(checks.rollup().state(), RollupState::Running);
    }

    #[test]
    fn rollups_rank_failure_over_running_over_success() {
        use CheckStatus as S;
        assert_eq!(
            Rollup::of([S::Success, S::InProgress, S::TimedOut]).state(),
            RollupState::Failing
        );
        assert_eq!(
            Rollup::of([S::Success, S::Queued]).state(),
            RollupState::Running
        );
        assert_eq!(
            Rollup::of([S::Success, S::Skipped]).state(),
            RollupState::Passing
        );
        assert_eq!(
            Rollup::of([S::Skipped, S::Cancelled]).state(),
            RollupState::Settled
        );
        assert_eq!(Rollup::of([]).state(), RollupState::Empty);
        assert_eq!(
            Rollup::of([S::Failure, S::Failure, S::InProgress, S::Success]).describe(),
            "2 failing · 1 running · 1 passed"
        );
        assert_eq!(Rollup::of([]).describe(), "no checks");
    }

    #[test]
    fn annotations_name_their_place() {
        let mut note = Annotation {
            path: "src/lib.rs".into(),
            start_line: 12,
            end_line: 12,
            level: AnnotationLevel::Failure,
            title: None,
            message: "boom".into(),
        };
        assert_eq!(note.location(), "src/lib.rs:12");
        note.end_line = 14;
        assert_eq!(note.location(), "src/lib.rs:12-14");
    }
}
