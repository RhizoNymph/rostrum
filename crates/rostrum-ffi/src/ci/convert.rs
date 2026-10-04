//! `rostrum_core::ci` values as the records Kotlin draws. Every rule —
//! columns, order, rollups, timing, log parsing, re-run eligibility — is
//! the core's; this only renames and adds colour roles.

use chrono::{DateTime, Utc};
use rostrum_core::{
    RepoId, StackPlace,
    ci::{
        Annotation, AnnotationLevel, CheckEntry, CheckKey, CheckOutput, CheckSource, CheckStatus,
        CiGrid as CoreGrid, GridLine, GridRow, LineKind, NotRerunnable, ParsedLog, RerunTarget,
        Rollup, RollupState, Timing,
    },
};

use crate::{
    ci::types::*,
    feed::{count, load_of},
    markdown::render,
    types::ColorRole,
};

impl From<CheckStatus> for CiStatus {
    fn from(status: CheckStatus) -> Self {
        match status {
            CheckStatus::Queued => Self::Queued,
            CheckStatus::InProgress => Self::InProgress,
            CheckStatus::Success => Self::Success,
            CheckStatus::Failure => Self::Failure,
            CheckStatus::Cancelled => Self::Cancelled,
            CheckStatus::Skipped => Self::Skipped,
            CheckStatus::Neutral => Self::Neutral,
            CheckStatus::TimedOut => Self::TimedOut,
            CheckStatus::ActionRequired => Self::ActionRequired,
        }
    }
}

fn status_role(status: CheckStatus) -> ColorRole {
    if status.is_failing() {
        ColorRole::Danger
    } else if status.is_running() {
        ColorRole::Warning
    } else if status == CheckStatus::Success {
        ColorRole::Success
    } else {
        ColorRole::Neutral
    }
}

impl From<&CheckKey> for CiCheckKey {
    fn from(key: &CheckKey) -> Self {
        Self {
            workflow: key.workflow.clone(),
            name: key.name.clone(),
        }
    }
}

impl From<CiCheckKey> for CheckKey {
    fn from(key: CiCheckKey) -> Self {
        CheckKey::new(key.workflow, key.name)
    }
}

fn source(source: &CheckSource) -> CiSource {
    match source {
        CheckSource::Actions {
            job_id,
            run_id,
            run_attempt,
            ..
        } => CiSource::Actions {
            job_id: *job_id,
            run_id: *run_id,
            run_attempt: *run_attempt,
        },
        CheckSource::App {
            check_run_id, app, ..
        } => CiSource::App {
            check_run_id: *check_run_id,
            app: app.clone(),
        },
        CheckSource::Status => CiSource::Status,
    }
}

pub(crate) fn cell(entry: &CheckEntry, now: DateTime<Utc>) -> CiCell {
    let timing = Timing::of(entry, now);
    CiCell {
        status: entry.status.into(),
        status_label: entry.status.label().to_string(),
        role: status_role(entry.status),
        timing_label: timing.label(),
        duration_label: timing.duration_label(),
        ticks: timing.ticks(),
        producer: entry.producer().to_string(),
        details_url: entry.details_url.clone(),
        source: source(&entry.source),
    }
}

fn rollup(rollup: &Rollup) -> CiRollup {
    let (state, role) = match rollup.state() {
        RollupState::Failing => (CiRollupState::Failing, ColorRole::Danger),
        RollupState::Running => (CiRollupState::Running, ColorRole::Warning),
        RollupState::Passing => (CiRollupState::Passing, ColorRole::Success),
        RollupState::Settled => (CiRollupState::Settled, ColorRole::Neutral),
        RollupState::Empty => (CiRollupState::Empty, ColorRole::Neutral),
    };
    CiRollup {
        failing: count(rollup.failing),
        running: count(rollup.running),
        passing: count(rollup.passing),
        other: count(rollup.other),
        state,
        label: rollup.describe(),
        role,
    }
}

fn place(place: StackPlace) -> CiStackPlace {
    match place {
        StackPlace::Bottom => CiStackPlace::Bottom,
        StackPlace::Middle => CiStackPlace::Middle,
        StackPlace::Top => CiStackPlace::Top,
        StackPlace::Only => CiStackPlace::Only,
    }
}

fn row(row: &GridRow, now: DateTime<Utc>) -> CiRow {
    CiRow {
        number: row.number.0,
        title: row.title.clone(),
        head_sha: row.head_sha.clone(),
        rollup: rollup(&row.rollup),
        cells: row
            .cells
            .iter()
            .map(|entry| entry.as_ref().map(|entry| cell(entry, now)))
            .collect(),
        stack: row.stack.map(place),
        fetched: row.fetched,
        truncated: row.truncated,
    }
}

/// The grid as of `now`.
pub(crate) fn grid(core: &CoreGrid, any_running: bool, now: DateTime<Utc>) -> CiGrid {
    let sections: Vec<CiSection> = core
        .sections()
        .iter()
        .map(|section| CiSection {
            repo: section.repo.to_string(),
            columns: section
                .columns
                .iter()
                .map(|key| CiColumn {
                    key: key.into(),
                    label: key.to_string(),
                })
                .collect(),
            rows: section.rows.iter().map(|r| row(r, now)).collect(),
            load: load_of(&section.load),
            hidden: count(section.hidden),
        })
        .collect();
    let ticks = sections
        .iter()
        .flat_map(|section| &section.rows)
        .flat_map(|row| &row.cells)
        .flatten()
        .any(|cell| cell.ticks);
    let lines = core
        .lines()
        .iter()
        .map(|line| match *line {
            GridLine::Header { section } => CiLine::Header {
                section: count(section),
            },
            GridLine::Stack { section, members } => CiLine::Stack {
                section: count(section),
                members: count(members),
            },
            GridLine::Row { section, row } => CiLine::Row {
                section: count(section),
                row: count(row),
            },
            GridLine::Notice { section } => CiLine::Notice {
                section: count(section),
            },
            GridLine::Spacer => CiLine::Spacer,
        })
        .collect();
    CiGrid {
        sections,
        lines,
        ticks,
        any_running,
    }
}

fn line_kind(kind: LineKind) -> CiLineKind {
    match kind {
        LineKind::Plain => CiLineKind::Plain,
        LineKind::GroupHeader => CiLineKind::GroupHeader,
        LineKind::Error => CiLineKind::Error,
        LineKind::Warning => CiLineKind::Warning,
        LineKind::Notice => CiLineKind::Notice,
        LineKind::Debug => CiLineKind::Debug,
        LineKind::Command => CiLineKind::Command,
    }
}

pub(crate) fn job_log(log: &ParsedLog) -> CiJobLog {
    CiJobLog {
        lines: log
            .lines
            .iter()
            .map(|line| CiLogLine {
                number: count(line.number),
                text: line.text.clone(),
                kind: line_kind(line.kind),
            })
            .collect(),
        groups: log
            .groups
            .iter()
            .map(|group| CiLogGroup {
                title: group.title.clone(),
                header: count(group.header),
                end: count(group.end),
            })
            .collect(),
        steps: log
            .steps
            .iter()
            .map(|step| CiLogStep {
                title: step.title.clone(),
                start: count(step.start),
                end: count(step.end),
            })
            .collect(),
        first_error: log.first_error.map(count),
        failing_step: log.failing_step.map(count),
        collapsed: log.default_collapsed().into_iter().map(count).collect(),
        dropped: count(log.dropped),
        truncated: log.dropped > 0,
    }
}

fn annotation(annotation: &Annotation) -> CiAnnotation {
    CiAnnotation {
        path: annotation.path.clone(),
        start_line: annotation.start_line,
        end_line: annotation.end_line,
        level: match annotation.level {
            AnnotationLevel::Notice => CiAnnotationLevel::Notice,
            AnnotationLevel::Warning => CiAnnotationLevel::Warning,
            AnnotationLevel::Failure => CiAnnotationLevel::Failure,
        },
        title: annotation.title.clone().filter(|title| !title.is_empty()),
        message: annotation.message.clone(),
        location: annotation.location(),
    }
}

/// Markdown flattened as the timeline flattens it.
pub(crate) fn check_output(output: &CheckOutput, repo: &RepoId) -> CiCheckOutput {
    let markdown = |text: &Option<String>| {
        text.as_deref()
            .map(|text| render(text, repo))
            .unwrap_or_default()
    };
    CiCheckOutput {
        title: output.title.clone(),
        summary: markdown(&output.summary),
        text: markdown(&output.text),
        annotations: output.annotations.iter().map(annotation).collect(),
    }
}

impl From<RerunTarget> for CiRerun {
    fn from(target: RerunTarget) -> Self {
        match target {
            RerunTarget::Job { job_id } => Self::Job { job_id },
            RerunTarget::FailedJobs { run_id } => Self::FailedJobs { run_id },
            RerunTarget::AllJobs { run_id } => Self::AllJobs { run_id },
            RerunTarget::Suite { suite_id } => Self::Suite { suite_id },
        }
    }
}

impl From<CiRerun> for RerunTarget {
    fn from(target: CiRerun) -> Self {
        match target {
            CiRerun::Job { job_id } => Self::Job { job_id },
            CiRerun::FailedJobs { run_id } => Self::FailedJobs { run_id },
            CiRerun::AllJobs { run_id } => Self::AllJobs { run_id },
            CiRerun::Suite { suite_id } => Self::Suite { suite_id },
        }
    }
}

/// What `entry` offers, given the rest of its pull request's checks.
pub(crate) fn rerun_choice(
    offered: Result<Vec<RerunTarget>, NotRerunnable>,
    entry: &CheckEntry,
) -> CiRerunChoice {
    match offered {
        Ok(targets) => CiRerunChoice::Available {
            options: targets
                .into_iter()
                .map(|target| CiRerunOption {
                    rerun: target.into(),
                    label: target.label().to_string(),
                    confirm_prompt: target.confirm_prompt(entry),
                })
                .collect(),
        },
        Err(reason) => CiRerunChoice::Unavailable {
            message: reason.to_string(),
            reason: match reason {
                NotRerunnable::StillRunning => CiNotRerunnable::StillRunning,
                NotRerunnable::LegacyStatus => CiNotRerunnable::LegacyStatus,
                NotRerunnable::NoSuite => CiNotRerunnable::NoSuite,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::ci::{LineLimit, parse_log};

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("time")
    }

    fn entry(status: CheckStatus, started: Option<i64>, completed: Option<i64>) -> CheckEntry {
        CheckEntry {
            key: CheckKey::new(Some("CI".into()), "build"),
            status,
            started_at: started.map(at),
            completed_at: completed.map(at),
            details_url: None,
            source: CheckSource::Actions {
                job_id: 1,
                run_id: 2,
                run_attempt: 1,
                suite_id: 3,
            },
        }
    }

    #[test]
    fn cells_carry_the_cores_timing_and_a_colour_role() {
        let running = cell(&entry(CheckStatus::InProgress, Some(0), None), at(192));
        assert_eq!(running.timing_label.as_deref(), Some("3m 12s"));
        assert_eq!(
            running.duration_label.as_deref(),
            Some("running for 3m 12s")
        );
        assert!(running.ticks);
        assert_eq!(running.role, ColorRole::Warning);

        let failed = cell(
            &entry(CheckStatus::Failure, Some(0), Some(243)),
            at(243 + 14 * 60),
        );
        assert_eq!(failed.timing_label.as_deref(), Some("finished 14m ago"));
        assert_eq!(failed.duration_label.as_deref(), Some("took 4m 03s"));
        assert!(!failed.ticks);
        assert_eq!(failed.role, ColorRole::Danger);
        assert_eq!(failed.status_label, "failure");

        let skipped = cell(&entry(CheckStatus::Skipped, None, None), at(0));
        assert_eq!(skipped.role, ColorRole::Neutral);
        assert_eq!(skipped.timing_label, None);
    }

    #[test]
    fn rollups_rank_failing_over_running_over_passing() {
        let failing = rollup(&Rollup::of([CheckStatus::Failure, CheckStatus::Queued]));
        assert_eq!(failing.state, CiRollupState::Failing);
        assert_eq!(failing.role, ColorRole::Danger);
        assert_eq!(rollup(&Rollup::of([])).state, CiRollupState::Empty);
        assert_eq!(
            rollup(&Rollup::of([CheckStatus::Success])).role,
            ColorRole::Success
        );
    }

    #[test]
    fn a_log_cut_to_its_tail_says_so() {
        let raw: String = (0..30).map(|n| format!("line {n}\n")).collect();
        let cut = job_log(&parse_log(&raw, LineLimit::Last(10)));
        assert!(cut.truncated);
        assert_eq!(cut.dropped, 20);
        assert_eq!(cut.lines.len(), 10);
        assert_eq!(cut.lines[0].number, 21);
        let full = job_log(&parse_log(&raw, LineLimit::Full));
        assert!(!full.truncated);
        assert_eq!(full.lines.len(), 30);
    }
}
