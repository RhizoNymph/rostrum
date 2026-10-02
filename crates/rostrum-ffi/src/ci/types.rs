//! Records for the CI grid, logs, check output and re-runs.

use crate::{feed::RepoLoad, markdown::MdBlock, types::ColorRole};

/// The grid's own narrowing, on top of the feed's filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CiGridFilter {
    /// Only pull requests with something failing or still running.
    pub needs_attention: bool,
}

/// One check's state, folded from GitHub's status and conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiStatus {
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

/// A column: an Actions job's workflow and name, or an app's or status's
/// name alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct CiCheckKey {
    pub workflow: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiColumn {
    pub key: CiCheckKey,
    /// `CI / build`, or `Coverage`.
    pub label: String,
}

/// Who produced a check, and the ids its log, output and re-runs take.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CiSource {
    /// A GitHub Actions job: `job_log(job_id)`.
    Actions {
        job_id: u64,
        run_id: u64,
        run_attempt: u32,
    },
    /// Another app's check run: `check_output(check_run_id)`.
    App { check_run_id: u64, app: String },
    /// A legacy commit status: only its link.
    Status,
}

/// One cell: a check on one pull request.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiCell {
    pub status: CiStatus,
    /// `in progress`, `failure`, …
    pub status_label: String,
    pub role: ColorRole,
    /// `3m 12s`, `queued 2m 00s`, `finished 14m ago`, as of when the grid
    /// was built.
    pub timing_label: Option<String>,
    /// `running for 3m 12s`, `took 4m 03s`.
    pub duration_label: Option<String>,
    /// The labels change every second: redraw while it does.
    pub ticks: bool,
    /// `GitHub Actions`, the app's name, or `the status provider`.
    pub producer: String,
    pub details_url: Option<String>,
    pub source: CiSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiRollupState {
    Failing,
    Running,
    Passing,
    /// Only skipped, cancelled or neutral checks.
    Settled,
    /// No checks at all.
    Empty,
}

/// A pull request's checks in one word.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiRollup {
    pub failing: u32,
    pub running: u32,
    pub passing: u32,
    pub other: u32,
    pub state: CiRollupState,
    /// `2 failing · 1 running`, or `no checks`.
    pub label: String,
    pub role: ColorRole,
}

/// Where a row sits in a stack the feed groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiStackPlace {
    Bottom,
    Middle,
    Top,
    Only,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiRow {
    pub number: u32,
    pub title: String,
    /// The head commit, seven characters.
    pub head_sha: String,
    pub rollup: CiRollup,
    /// One per section column; `None` is "not run" on this pull request.
    pub cells: Vec<Option<CiCell>>,
    pub stack: Option<CiStackPlace>,
    /// Whether this pull request's checks have been fetched at all.
    pub fetched: bool,
    /// GitHub reported more checks than were fetched ("more not shown").
    pub truncated: bool,
}

/// One repository's block of the grid, with its own columns.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiSection {
    pub repo: String,
    pub columns: Vec<CiColumn>,
    pub rows: Vec<CiRow>,
    pub load: RepoLoad,
    /// Rows the grid filter removed.
    pub hidden: u32,
}

/// A line of the grid flattened for one list, as the desktop draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiLine {
    /// The repository's name and column headers.
    Header {
        section: u32,
    },
    /// Heads a stack's rows: "Stack · 3 PRs".
    Stack {
        section: u32,
        members: u32,
    },
    Row {
        section: u32,
        row: u32,
    },
    /// The section has no rows: loading, failed, or nothing open.
    Notice {
        section: u32,
    },
    Spacer,
}

/// The PRs × checks matrix, in the feed's order.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiGrid {
    pub sections: Vec<CiSection>,
    pub lines: Vec<CiLine>,
    /// Some cell's labels change every second: rebuild the grid (`ci_grid`,
    /// no network) once a second while it is on screen.
    pub ticks: bool,
    /// Something anywhere is queued or running: worth re-fetching
    /// (`refresh_ci`) every 15 s while the grid is on screen.
    pub any_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiLineKind {
    Plain,
    /// The title of a collapsible group.
    GroupHeader,
    Error,
    Warning,
    Notice,
    Debug,
    /// A `[command]` echo of what the runner executed.
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiLogLine {
    /// The line's number in the full log, from 1.
    pub number: u32,
    /// Timestamp, ANSI and workflow-command markers removed.
    pub text: String,
    pub kind: CiLineKind,
}

/// A collapsible group: `header` and every line up to `end` (exclusive),
/// as indices into `lines`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiLogGroup {
    pub title: String,
    pub header: u32,
    pub end: u32,
}

/// One step of the job: lines `start..end`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiLogStep {
    pub title: String,
    pub start: u32,
    pub end: u32,
}

/// An Actions job's log, parsed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiJobLog {
    pub lines: Vec<CiLogLine>,
    pub groups: Vec<CiLogGroup>,
    pub steps: Vec<CiLogStep>,
    /// Index into `lines` of the first error.
    pub first_error: Option<u32>,
    /// Index into `steps` of the step holding it.
    pub failing_step: Option<u32>,
    /// Indices into `groups` to start collapsed: all but one holding an
    /// error.
    pub collapsed: Vec<u32>,
    /// Lines dropped from the top of a long log.
    pub dropped: u32,
    /// The log was cut to its tail; `job_log(.., full = true)` has it all.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiAnnotationLevel {
    Notice,
    Warning,
    Failure,
}

/// A note a check run attached to lines of a file.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiAnnotation {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub level: CiAnnotationLevel,
    pub title: Option<String>,
    pub message: String,
    /// `src/lib.rs:12` or `src/lib.rs:12-14`.
    pub location: String,
}

/// Another app's check run: its output as markdown, and its annotations.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CiCheckOutput {
    pub title: Option<String>,
    pub summary: Vec<MdBlock>,
    pub text: Vec<MdBlock>,
    pub annotations: Vec<CiAnnotation>,
}

/// A re-run the user can ask for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum CiRerun {
    Job {
        job_id: u64,
    },
    /// Every failed or cancelled job of a workflow run.
    FailedJobs {
        run_id: u64,
    },
    AllJobs {
        run_id: u64,
    },
    /// Ask another app to run its check suite again.
    Suite {
        suite_id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiRerunOption {
    pub rerun: CiRerun,
    /// `Re-run failed jobs`.
    pub label: String,
    /// The confirmation's question, naming what restarts.
    pub confirm_prompt: String,
}

/// Why a check offers no re-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiNotRerunnable {
    /// Its workflow run (or the check) has not finished.
    StillRunning,
    /// A legacy commit status: only the service that posted it can re-run it.
    LegacyStatus,
    /// Another app's check without a suite to re-request.
    NoSuite,
}

/// What a check offers, first option primary.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CiRerunChoice {
    Available {
        options: Vec<CiRerunOption>,
    },
    Unavailable {
        reason: CiNotRerunnable,
        message: String,
    },
}
