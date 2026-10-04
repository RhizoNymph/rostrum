//! The CI grid on the phone, end to end against the GitHub stand-in: the
//! matrix in the feed's order with timing labels and the tick hint, job logs,
//! another app's check output, re-run eligibility, and re-runs with their
//! optimistic flip and typed refusals.

mod support;

use std::sync::Arc;

use chrono::{Duration, Utc};
use rostrum_config::Config;
use rostrum_ffi::{
    RostrumCore, RostrumError,
    ci::{
        CiCell, CiCheckKey, CiGrid, CiGridFilter, CiLine, CiLineKind, CiNotRerunnable, CiRerun,
        CiRerunChoice, CiRollupState, CiSource, CiStatus,
    },
    types::ColorRole,
};
use serde_json::{Value, json};
use support::{
    Scratch,
    github::{FakeGitHub, Pr, World},
};

fn ago(minutes: i64) -> String {
    (Utc::now() - Duration::minutes(minutes)).to_rfc3339()
}

/// An Actions job of workflow "CI".
fn job(
    id: u64,
    run: u64,
    name: &str,
    status: &str,
    conclusion: Option<&str>,
    started: i64,
    completed: Option<i64>,
) -> Value {
    json!({
        "__typename": "CheckRun",
        "databaseId": id,
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "startedAt": ago(started),
        "completedAt": completed.map(ago),
        "detailsUrl": format!("https://github.com/octo/repo/actions/runs/{run}/job/{id}"),
        "checkSuite": {
            "databaseId": 900 + run,
            "app": {"name": "GitHub Actions", "slug": "github-actions"},
            "workflowRun": {"databaseId": run, "runNumber": 7, "runAttempt": 1, "workflow": {"name": "CI"}}
        }
    })
}

fn world() -> World {
    World {
        viewer: "me".into(),
        repos: vec![(
            "octo/repo".into(),
            vec![Pr::new(1, "alice"), Pr::new(2, "bob"), Pr::new(3, "carol")],
        )],
        ci: vec![
            (
                1,
                vec![
                    job(11, 100, "build", "COMPLETED", Some("SUCCESS"), 20, Some(16)),
                    job(12, 100, "test", "COMPLETED", Some("FAILURE"), 20, Some(14)),
                    json!({
                        "__typename": "CheckRun", "databaseId": 21, "name": "Coverage",
                        "status": "COMPLETED", "conclusion": "NEUTRAL",
                        "startedAt": ago(10), "completedAt": ago(9),
                        "detailsUrl": "https://coverage.example/21",
                        "checkSuite": {"databaseId": 30, "app": {"name": "Coverage Bot", "slug": "cov"}, "workflowRun": null}
                    }),
                    json!({
                        "__typename": "StatusContext", "context": "deploy/preview",
                        "state": "SUCCESS", "createdAt": ago(5),
                        "targetUrl": "https://preview.example", "description": "ok"
                    }),
                ],
            ),
            (2, vec![job(13, 101, "build", "IN_PROGRESS", None, 1, None)]),
            (
                3,
                vec![job(
                    14,
                    102,
                    "build",
                    "COMPLETED",
                    Some("SUCCESS"),
                    30,
                    Some(25),
                )],
            ),
        ],
        ..Default::default()
    }
}

async fn core_against(fake: &FakeGitHub, scratch: &Scratch) -> Arc<RostrumCore> {
    Config {
        repos: vec!["octo/repo".into()],
        ..Default::default()
    }
    .save_to(&scratch.dir.join("config.json"))
    .expect("config");
    let core = RostrumCore::open_with_github_api(
        scratch.path(),
        fake.graphql_url.clone(),
        fake.rest_base.clone(),
    )
    .await
    .expect("open");
    core.set_github_token(Some("ghp_test".into()))
        .await
        .expect("token");
    core.refresh_feed().await.expect("feed");
    core
}

fn key(workflow: Option<&str>, name: &str) -> CiCheckKey {
    CiCheckKey {
        workflow: workflow.map(Into::into),
        name: name.into(),
    }
}

/// The cell in row `number` under `column`.
fn cell<'a>(grid: &'a CiGrid, number: u32, column: &CiCheckKey) -> Option<&'a CiCell> {
    let section = &grid.sections[0];
    let ix = section
        .columns
        .iter()
        .position(|candidate| &candidate.key == column)?;
    section.rows.iter().find(|row| row.number == number)?.cells[ix].as_ref()
}

fn none() -> CiGridFilter {
    CiGridFilter {
        needs_attention: false,
    }
}

#[tokio::test]
async fn the_grid_follows_the_feed_with_timing_and_a_tick_hint() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("ci-grid");
    let core = core_against(&fake, &scratch).await;

    // Nothing fetched yet: rows from the feed, no cells.
    let empty = core.ci_grid(none()).await.expect("grid");
    assert_eq!(empty.sections[0].rows.len(), 3);
    assert!(empty.sections[0].rows.iter().all(|row| !row.fetched));
    assert!(!empty.ticks);

    let grid = core.refresh_ci(none()).await.expect("refresh");
    let section = &grid.sections[0];
    assert_eq!(section.repo, "octo/repo");
    assert_eq!(
        section
            .columns
            .iter()
            .map(|column| column.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Coverage", "deploy/preview", "CI / build", "CI / test"]
    );
    assert_eq!(
        section
            .rows
            .iter()
            .map(|row| row.number)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(matches!(grid.lines[0], CiLine::Header { section: 0 }));

    let row = &section.rows[0];
    assert!(row.fetched);
    assert_eq!(row.head_sha, "sha1");
    assert_eq!(row.rollup.state, CiRollupState::Failing);
    assert_eq!(row.rollup.role, ColorRole::Danger);

    let test = cell(&grid, 1, &key(Some("CI"), "test")).expect("test cell");
    assert_eq!(test.status, CiStatus::Failure);
    assert_eq!(test.role, ColorRole::Danger);
    assert_eq!(test.timing_label.as_deref(), Some("finished 14m ago"));
    assert_eq!(test.duration_label.as_deref(), Some("took 6m 00s"));
    assert!(!test.ticks);
    assert!(matches!(
        test.source,
        CiSource::Actions {
            job_id: 12,
            run_id: 100,
            ..
        }
    ));
    let coverage = cell(&grid, 1, &key(None, "Coverage")).expect("coverage");
    assert_eq!(coverage.producer, "Coverage Bot");
    assert!(matches!(
        coverage.source,
        CiSource::App {
            check_run_id: 21,
            ..
        }
    ));
    assert!(matches!(
        cell(&grid, 1, &key(None, "deploy/preview"))
            .expect("status")
            .source,
        CiSource::Status
    ));
    // Not run on #2.
    assert!(cell(&grid, 2, &key(Some("CI"), "test")).is_none());

    let running = cell(&grid, 2, &key(Some("CI"), "build")).expect("running");
    assert_eq!(running.status, CiStatus::InProgress);
    assert!(running.ticks);
    assert!(
        running
            .timing_label
            .as_deref()
            .is_some_and(|label| label.starts_with("1m ")),
        "{:?}",
        running.timing_label
    );
    assert!(grid.ticks, "a running cell ticks");
    assert!(grid.any_running);

    // Failing or running only: #3 (all passing) is hidden; columns stay.
    let attention = core
        .ci_grid(CiGridFilter {
            needs_attention: true,
        })
        .await
        .expect("filtered");
    assert_eq!(
        attention.sections[0]
            .rows
            .iter()
            .map(|row| row.number)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(attention.sections[0].hidden, 1);
    assert_eq!(attention.sections[0].columns.len(), 4);

    // Refreshing one repository works the same way.
    let one = core
        .refresh_ci_repo("octo/repo".into(), none())
        .await
        .expect("one");
    assert_eq!(one.sections[0].rows.len(), 3);
    assert_eq!(fake.graphql_calls("contexts(first").len(), 2);
}

#[tokio::test]
async fn a_job_log_is_parsed_and_another_apps_output_rendered() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("ci-logs");
    let core = core_against(&fake, &scratch).await;
    core.refresh_ci(none()).await.expect("refresh");

    let log = core
        .job_log("octo/repo".into(), 12, false)
        .await
        .expect("log");
    assert!(!log.lines.is_empty());
    assert!(!log.truncated);
    assert_eq!(log.dropped, 0);
    let first_error = log.first_error.expect("an error line") as usize;
    assert_eq!(log.lines[first_error].kind, CiLineKind::Error);
    assert_eq!(
        log.lines[first_error].text,
        "Process completed with exit code 2."
    );
    let failing = log.failing_step.expect("a failing step") as usize;
    assert!(
        log.steps[failing].title.starts_with("Run "),
        "{}",
        log.steps[failing].title
    );
    // Groups start collapsed, except one holding the error.
    assert!(!log.groups.is_empty());
    assert!(log.collapsed.iter().all(|group| {
        let group = &log.groups[*group as usize];
        !(group.header as usize..group.end as usize).contains(&first_error)
    }));
    assert!(log.lines.iter().all(|line| !line.text.contains("##[")));

    // The full log re-parses the text already fetched.
    let full = core
        .job_log("octo/repo".into(), 12, true)
        .await
        .expect("full");
    assert_eq!(full.lines.len(), log.lines.len());
    assert_eq!(
        fake.log
            .requests()
            .iter()
            .filter(|request| request.path.ends_with("/actions/jobs/12/logs"))
            .count(),
        1
    );

    let output = core
        .check_output("octo/repo".into(), 21)
        .await
        .expect("output");
    assert_eq!(output.title.as_deref(), Some("Awaiting approvals (0/1)"));
    assert!(!output.summary.is_empty());
    assert_eq!(output.annotations.len(), 1);
    assert_eq!(
        output.annotations[0].message,
        "Process completed with exit code 2."
    );
    assert!(output.annotations[0].location.starts_with(".github"));
}

#[tokio::test]
async fn re_runs_are_offered_by_the_rules_and_flip_optimistically() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("ci-rerun");
    let core = core_against(&fake, &scratch).await;
    core.refresh_ci(none()).await.expect("refresh");

    let CiRerunChoice::Available { options } = core
        .rerun_targets("octo/repo".into(), 1, key(Some("CI"), "test"))
        .await
        .expect("targets")
    else {
        panic!("a finished failed job can be re-run");
    };
    assert_eq!(
        options
            .iter()
            .map(|option| option.rerun.clone())
            .collect::<Vec<_>>(),
        vec![
            CiRerun::Job { job_id: 12 },
            CiRerun::FailedJobs { run_id: 100 },
            CiRerun::AllJobs { run_id: 100 },
        ]
    );
    assert_eq!(options[1].label, "Re-run failed jobs");
    assert_eq!(options[1].confirm_prompt, "Re-run the failed jobs of CI?");
    for (pr, column, reason) in [
        (2, key(Some("CI"), "build"), CiNotRerunnable::StillRunning),
        (
            1,
            key(None, "deploy/preview"),
            CiNotRerunnable::LegacyStatus,
        ),
    ] {
        let choice = core
            .rerun_targets("octo/repo".into(), pr, column)
            .await
            .expect("targets");
        assert!(
            matches!(&choice, CiRerunChoice::Unavailable { reason: r, .. } if *r == reason),
            "{choice:?}"
        );
    }
    assert!(matches!(
        core.rerun_targets("octo/repo".into(), 1, key(None, "nothing"))
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));

    core.rerun("octo/repo".into(), CiRerun::FailedJobs { run_id: 100 })
        .await
        .expect("rerun");
    assert_eq!(
        fake.rest_calls(
            "POST",
            "/repos/octo/repo/actions/runs/100/rerun-failed-jobs"
        )
        .len(),
        1
    );
    // The failed job shows queued at once; the passing one is untouched.
    let flipped = core.ci_grid(none()).await.expect("grid");
    let test = cell(&flipped, 1, &key(Some("CI"), "test")).expect("test");
    assert_eq!(test.status, CiStatus::Queued);
    assert_eq!(test.timing_label.as_deref(), Some("queued"));
    assert_eq!(
        cell(&flipped, 1, &key(Some("CI"), "build"))
            .expect("build")
            .status,
        CiStatus::Success
    );
}

#[tokio::test]
async fn refused_re_runs_are_typed_and_put_the_old_result_back() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("ci-refused");
    let core = core_against(&fake, &scratch).await;
    core.refresh_ci(none()).await.expect("refresh");
    let reads = fake.graphql_calls("contexts(first").len();

    for (answer, expected) in [
        (
            (
                403,
                json!({"message": "Must have admin rights to Repository."}),
            ),
            "no permission",
        ),
        (
            (
                403,
                json!({"message": "This workflow run cannot be retried. It was created over a month ago."}),
            ),
            "not rerunnable",
        ),
        ((409, json!({"message": "conflict"})), "not rerunnable"),
        ((404, json!({"message": "Not Found"})), "not found"),
    ] {
        fake.edit(|world| world.rerun = Some((answer.0, answer.1.to_string())));
        let error = core
            .rerun("octo/repo".into(), CiRerun::Job { job_id: 12 })
            .await
            .expect_err("refused");
        let kind = match &error {
            RostrumError::CiNoPermission { .. } => "no permission",
            RostrumError::CiNotRerunnable { .. } => "not rerunnable",
            RostrumError::CiNotFound => "not found",
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(kind, expected, "{error:?}");
        // Re-fetched at once: the failure is back.
        let grid = core.ci_grid(none()).await.expect("grid");
        assert_eq!(
            cell(&grid, 1, &key(Some("CI"), "test"))
                .expect("test")
                .status,
            CiStatus::Failure
        );
    }
    assert_eq!(fake.graphql_calls("contexts(first").len(), reads + 4);
}
