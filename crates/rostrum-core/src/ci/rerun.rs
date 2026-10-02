//! Which re-runs a check offers, and what the grid shows once one is asked
//! for.

use super::model::{CheckEntry, CheckSource, CheckStatus, PrChecks};

/// A re-run the user can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RerunTarget {
    /// One Actions job (`POST /actions/jobs/{id}/rerun`).
    Job { job_id: u64 },
    /// Every failed or cancelled job of a workflow run
    /// (`POST /actions/runs/{id}/rerun-failed-jobs`).
    FailedJobs { run_id: u64 },
    /// The whole workflow run (`POST /actions/runs/{id}/rerun`).
    AllJobs { run_id: u64 },
    /// A third-party app's check suite (`POST /check-suites/{id}/rerequest`).
    Suite { suite_id: u64 },
}

impl RerunTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Job { .. } => "Re-run job",
            Self::FailedJobs { .. } => "Re-run failed jobs",
            Self::AllJobs { .. } => "Re-run all jobs",
            Self::Suite { .. } => "Re-request",
        }
    }

    /// The confirmation's question, naming what is about to restart.
    pub fn confirm_prompt(self, entry: &CheckEntry) -> String {
        match self {
            Self::Job { .. } => format!("Re-run the job “{}”?", entry.key),
            Self::FailedJobs { .. } => format!(
                "Re-run the failed jobs of {}?",
                entry.key.workflow.as_deref().unwrap_or("this workflow run")
            ),
            Self::AllJobs { .. } => format!(
                "Re-run every job of {}?",
                entry.key.workflow.as_deref().unwrap_or("this workflow run")
            ),
            Self::Suite { .. } => format!("Ask {} to run “{}” again?", entry.producer(), entry.key),
        }
    }

    /// Whether `entry` is one of the checks this re-run restarts.
    fn covers(self, entry: &CheckEntry) -> bool {
        match (self, &entry.source) {
            (Self::Job { job_id }, CheckSource::Actions { job_id: id, .. }) => *id == job_id,
            (Self::FailedJobs { run_id }, CheckSource::Actions { run_id: id, .. }) => {
                *id == run_id
                    && (entry.status.is_failing() || entry.status == CheckStatus::Cancelled)
            }
            (Self::AllJobs { run_id }, CheckSource::Actions { run_id: id, .. }) => *id == run_id,
            (
                Self::Suite { suite_id },
                CheckSource::App {
                    suite_id: Some(id), ..
                },
            ) => *id == suite_id,
            _ => false,
        }
    }
}

/// Why a check offers no re-run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotRerunnable {
    /// GitHub refuses to re-run a workflow run that has not finished.
    #[error("the workflow run is still in progress")]
    StillRunning,
    /// A legacy commit status belongs to an outside service with no re-run
    /// API on GitHub's side.
    #[error("a commit status can only be re-run by the service that posted it")]
    LegacyStatus,
    /// A third-party check run without a check suite to re-request.
    #[error("this check has no check suite to re-request")]
    NoSuite,
}

/// The re-runs `entry` offers, given the rest of its pull request's checks.
///
/// Actions: nothing while any job of the same workflow run is still queued or
/// running — GitHub refuses those requests. Once the run is done, the job
/// itself, the run's failed jobs when it has any failed or cancelled, and the
/// whole run. A third-party check offers re-requesting its suite once it has
/// finished; whether the app allows it is the app's call, and a refusal comes
/// back as an error. A legacy status offers nothing.
pub fn rerun_targets(
    checks: &PrChecks,
    entry: &CheckEntry,
) -> Result<Vec<RerunTarget>, NotRerunnable> {
    match &entry.source {
        CheckSource::Status => Err(NotRerunnable::LegacyStatus),
        CheckSource::App { suite_id: None, .. } => Err(NotRerunnable::NoSuite),
        CheckSource::App { .. } if entry.status.is_running() => Err(NotRerunnable::StillRunning),
        CheckSource::App {
            suite_id: Some(suite_id),
            ..
        } => Ok(vec![RerunTarget::Suite {
            suite_id: *suite_id,
        }]),
        CheckSource::Actions { job_id, run_id, .. } => {
            let run: Vec<&CheckEntry> = checks
                .entries
                .iter()
                .filter(|other| other.run_id() == Some(*run_id))
                .collect();
            if run.iter().any(|job| job.status.is_running()) || entry.status.is_running() {
                return Err(NotRerunnable::StillRunning);
            }
            let mut targets = vec![RerunTarget::Job { job_id: *job_id }];
            let failed = RerunTarget::FailedJobs { run_id: *run_id };
            if run.iter().any(|job| failed.covers(job)) {
                targets.push(failed);
            }
            targets.push(RerunTarget::AllJobs { run_id: *run_id });
            Ok(targets)
        }
    }
}

/// Show the checks a re-run restarts as queued, before GitHub confirms.
///
/// Optimistic: the next fetch replaces the pull request's checks wholesale,
/// which is what reconciles it — with the new attempt, or, if the request
/// failed, with the old result again. Returns how many checks were marked.
pub fn mark_requeued(checks: &mut PrChecks, target: RerunTarget) -> usize {
    let mut marked = 0;
    for entry in checks
        .entries
        .iter_mut()
        .filter(|entry| target.covers(entry))
    {
        entry.status = CheckStatus::Queued;
        entry.started_at = None;
        entry.completed_at = None;
        marked += 1;
    }
    marked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ci::model::CheckKey, model::PrNumber};

    fn actions(name: &str, status: CheckStatus, job_id: u64, run_id: u64) -> CheckEntry {
        CheckEntry {
            key: CheckKey::new(Some("CI".into()), name),
            status,
            started_at: chrono::DateTime::from_timestamp(100, 0),
            completed_at: chrono::DateTime::from_timestamp(200, 0),
            details_url: None,
            source: CheckSource::Actions {
                job_id,
                run_id,
                run_attempt: 1,
                suite_id: 5,
            },
        }
    }

    fn app(status: CheckStatus, suite_id: Option<u64>) -> CheckEntry {
        CheckEntry {
            key: CheckKey::new(None, "Deploy preview"),
            status,
            started_at: None,
            completed_at: None,
            details_url: Some("https://example.invalid".into()),
            source: CheckSource::App {
                check_run_id: 77,
                suite_id,
                app: "Vercel".into(),
            },
        }
    }

    fn pr(entries: Vec<CheckEntry>) -> PrChecks {
        PrChecks {
            number: PrNumber(1),
            head_sha: "abc".into(),
            entries,
            truncated: false,
        }
    }

    #[test]
    fn a_finished_run_with_a_failure_offers_all_three() {
        let checks = pr(vec![
            actions("build", CheckStatus::Success, 1, 10),
            actions("test", CheckStatus::Failure, 2, 10),
        ]);
        assert_eq!(
            rerun_targets(&checks, &checks.entries[0]),
            Ok(vec![
                RerunTarget::Job { job_id: 1 },
                RerunTarget::FailedJobs { run_id: 10 },
                RerunTarget::AllJobs { run_id: 10 },
            ])
        );
    }

    #[test]
    fn a_green_run_has_no_failed_jobs_to_rerun() {
        let checks = pr(vec![actions("build", CheckStatus::Success, 1, 10)]);
        assert_eq!(
            rerun_targets(&checks, &checks.entries[0]),
            Ok(vec![
                RerunTarget::Job { job_id: 1 },
                RerunTarget::AllJobs { run_id: 10 }
            ])
        );
    }

    /// GitHub's "re-run failed jobs" restarts cancelled ones too.
    #[test]
    fn a_cancelled_job_counts_as_failed_for_rerunning() {
        let checks = pr(vec![actions("build", CheckStatus::Cancelled, 1, 10)]);
        assert!(
            rerun_targets(&checks, &checks.entries[0])
                .expect("rerunnable")
                .contains(&RerunTarget::FailedJobs { run_id: 10 })
        );
    }

    /// Any job of the same run still going blocks every re-run of it, even
    /// from a finished job; another run's jobs do not.
    #[test]
    fn a_run_still_going_offers_nothing() {
        let checks = pr(vec![
            actions("build", CheckStatus::Failure, 1, 10),
            actions("test", CheckStatus::InProgress, 2, 10),
            actions("lint", CheckStatus::Queued, 3, 11),
            actions("docs", CheckStatus::Failure, 4, 12),
        ]);
        assert_eq!(
            rerun_targets(&checks, &checks.entries[0]),
            Err(NotRerunnable::StillRunning)
        );
        assert_eq!(
            rerun_targets(&checks, &checks.entries[1]),
            Err(NotRerunnable::StillRunning)
        );
        assert!(rerun_targets(&checks, &checks.entries[3]).is_ok());
    }

    #[test]
    fn third_party_checks_offer_re_requesting_their_suite() {
        let done = app(CheckStatus::Failure, Some(55));
        assert_eq!(
            rerun_targets(&pr(vec![done.clone()]), &done),
            Ok(vec![RerunTarget::Suite { suite_id: 55 }])
        );
        let running = app(CheckStatus::InProgress, Some(55));
        assert_eq!(
            rerun_targets(&pr(vec![]), &running),
            Err(NotRerunnable::StillRunning)
        );
        let orphan = app(CheckStatus::Failure, None);
        assert_eq!(
            rerun_targets(&pr(vec![]), &orphan),
            Err(NotRerunnable::NoSuite)
        );
    }

    #[test]
    fn legacy_statuses_offer_nothing() {
        let status = CheckEntry {
            source: CheckSource::Status,
            ..app(CheckStatus::Failure, None)
        };
        assert_eq!(
            rerun_targets(&pr(vec![]), &status),
            Err(NotRerunnable::LegacyStatus)
        );
    }

    #[test]
    fn requeueing_marks_exactly_what_the_rerun_restarts() {
        let base = pr(vec![
            actions("build", CheckStatus::Success, 1, 10),
            actions("test", CheckStatus::Failure, 2, 10),
            actions("lint", CheckStatus::Cancelled, 3, 10),
            actions("other", CheckStatus::Failure, 4, 11),
        ]);
        let statuses = |checks: &PrChecks| -> Vec<CheckStatus> {
            checks.entries.iter().map(|e| e.status).collect()
        };
        use CheckStatus::*;

        let mut one = base.clone();
        assert_eq!(mark_requeued(&mut one, RerunTarget::Job { job_id: 2 }), 1);
        assert_eq!(statuses(&one), [Success, Queued, Cancelled, Failure]);
        assert_eq!(one.entries[1].started_at, None);

        let mut failed = base.clone();
        assert_eq!(
            mark_requeued(&mut failed, RerunTarget::FailedJobs { run_id: 10 }),
            2
        );
        assert_eq!(statuses(&failed), [Success, Queued, Queued, Failure]);

        let mut all = base.clone();
        assert_eq!(
            mark_requeued(&mut all, RerunTarget::AllJobs { run_id: 10 }),
            3
        );
        assert_eq!(statuses(&all), [Queued, Queued, Queued, Failure]);

        let mut suite = pr(vec![
            app(CheckStatus::Failure, Some(55)),
            actions("x", Failure, 9, 9),
        ]);
        assert_eq!(
            mark_requeued(&mut suite, RerunTarget::Suite { suite_id: 55 }),
            1
        );
        assert_eq!(statuses(&suite), [Queued, Failure]);
    }

    #[test]
    fn prompts_name_what_restarts() {
        let job = actions("test", CheckStatus::Failure, 2, 10);
        assert_eq!(
            RerunTarget::Job { job_id: 2 }.confirm_prompt(&job),
            "Re-run the job “CI / test”?"
        );
        assert_eq!(
            RerunTarget::FailedJobs { run_id: 10 }.confirm_prompt(&job),
            "Re-run the failed jobs of CI?"
        );
        assert_eq!(
            RerunTarget::Suite { suite_id: 1 }.confirm_prompt(&app(CheckStatus::Failure, Some(1))),
            "Ask Vercel to run “Deploy preview” again?"
        );
    }
}
