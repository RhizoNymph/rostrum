//! CI decoding and parsing against responses captured from public repos.
//!
//! - `checks_vscode_running.json`: microsoft/vscode, four open PRs with jobs
//!   in progress and third-party app checks.
//! - `checks_cpython_failed.json`: python/cpython, five open PRs, one with
//!   failed jobs and the same check name run by several workflow runs, plus
//!   legacy commit statuses.
//! - `checks_kubernetes_statuses.json`: kubernetes/kubernetes, legacy
//!   statuses only (Prow), some pending.
//! - `job_log_failed_excerpt.txt`: a balanced excerpt of a failed cpython
//!   Windows job's log (job 110642708500), BOM, timestamps and ANSI intact.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use rostrum_core::{
    FeedFilter, LoadState, PrNumber, RepoId, RepoState,
    ci::{
        CheckKey, CheckSource, CheckStatus, CiChecks, GridFilter, LineKind, LineLimit, PrChecks,
        RerunTarget, RollupState, Timing, build_grid, parse_log, rerun_targets,
    },
};
use serde_json::{Value, json};

use crate::{
    ci::wire::{CI_CHECKS, CONTEXTS_PER_COMMIT, CiChecksData},
    graphql::GraphQlResponse,
};

const VSCODE: &str = include_str!("../../fixtures/ci/checks_vscode_running.json");
const CPYTHON: &str = include_str!("../../fixtures/ci/checks_cpython_failed.json");
const KUBERNETES: &str = include_str!("../../fixtures/ci/checks_kubernetes_statuses.json");
const LOG: &str = include_str!("../../fixtures/ci/job_log_failed_excerpt.txt");

fn decode(body: &str) -> Vec<PrChecks> {
    let response: GraphQlResponse<CiChecksData> =
        serde_json::from_str(body).expect("fixture decodes");
    assert!(response.errors.is_empty());
    response
        .data
        .expect("data")
        .into_domain()
        .expect("repository present")
}

fn raw_contexts(body: &str) -> usize {
    let value: Value = serde_json::from_str(body).expect("json");
    value["data"]["repository"]["pullRequests"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|pr| {
            pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"]
                .as_array()
                .map_or(0, Vec::len)
        })
        .sum()
}

#[test]
fn the_document_asks_for_both_kinds_of_context() {
    for field in [
        "... on CheckRun",
        "databaseId",
        "startedAt",
        "completedAt",
        "detailsUrl",
        "workflowRun { databaseId runNumber runAttempt workflow { name } }",
        "app { name slug }",
        "... on StatusContext",
        "targetUrl",
        "totalCount",
    ] {
        assert!(CI_CHECKS.contains(field), "{field}");
    }
    assert!(CI_CHECKS.contains(&format!("contexts(first: {CONTEXTS_PER_COMMIT})")));
}

#[test]
fn every_captured_context_decodes() {
    for body in [VSCODE, CPYTHON, KUBERNETES] {
        let decoded: usize = decode(body).iter().map(|pr| pr.entries.len()).sum();
        assert_eq!(decoded, raw_contexts(body));
    }
}

#[test]
fn actions_jobs_carry_their_run_and_workflow() {
    let prs = decode(CPYTHON);
    let failed = prs
        .iter()
        .flat_map(|pr| &pr.entries)
        .find(|e| {
            matches!(
                e.source,
                CheckSource::Actions {
                    job_id: 110642708500,
                    ..
                }
            )
        })
        .expect("the failed Windows job");
    assert_eq!(failed.status, CheckStatus::Failure);
    assert_eq!(
        failed.key.name,
        "Windows (free-threading) / Build and test (x64, switch-case)"
    );
    assert!(failed.key.workflow.is_some());
    assert!(matches!(
        failed.source,
        CheckSource::Actions {
            run_id: 36944162426,
            run_attempt: 1,
            ..
        }
    ));
    assert!(failed.started_at.is_some() && failed.completed_at.is_some());
    assert!(
        failed
            .details_url
            .as_deref()
            .is_some_and(|u| u.contains("/job/"))
    );
}

#[test]
fn running_jobs_and_third_party_checks_decode() {
    let prs = decode(VSCODE);
    let entries: Vec<_> = prs.iter().flat_map(|pr| &pr.entries).collect();
    let running = entries
        .iter()
        .find(|e| e.status == CheckStatus::InProgress)
        .expect("a running job");
    assert!(running.started_at.is_some());
    assert_eq!(running.completed_at, None);

    let app = entries
        .iter()
        .find(|e| {
            matches!(
                &e.source,
                CheckSource::App {
                    check_run_id: 110657723167,
                    ..
                }
            )
        })
        .expect("the VS Code PR Check");
    let CheckSource::App {
        app: name,
        suite_id,
        ..
    } = &app.source
    else {
        unreachable!()
    };
    assert!(!name.is_empty());
    assert!(suite_id.is_some());
    assert_eq!(app.key, CheckKey::new(None, "VS Code PR Check"));
}

#[test]
fn legacy_statuses_decode_with_one_timestamp() {
    let prs = decode(KUBERNETES);
    let entries: Vec<_> = prs.iter().flat_map(|pr| &pr.entries).collect();
    assert!(entries.iter().all(|e| e.source == CheckSource::Status));
    let pending = entries
        .iter()
        .find(|e| e.status == CheckStatus::InProgress)
        .expect("a pending status");
    assert!(pending.started_at.is_some());
    assert_eq!(pending.completed_at, None);
    let done = entries
        .iter()
        .find(|e| e.status == CheckStatus::Success)
        .expect("a passing status");
    assert_eq!(done.completed_at, done.started_at);
    assert!(prs.iter().all(|pr| !pr.truncated));
}

/// Several workflow runs on one commit each report a check of the same name;
/// the grid keeps one per column, and only one.
#[test]
fn repeated_check_names_collapse_to_one_cell() {
    let prs = decode(CPYTHON);
    let pr = prs
        .iter()
        .find(|pr| pr.number == PrNumber(157932))
        .expect("#157932");
    let unresolved = pr
        .entries
        .iter()
        .filter(|e| e.key.name == "Unresolved review")
        .count();
    assert!(unresolved > 1, "the fixture repeats the name");
    let latest = pr.latest();
    assert_eq!(
        latest
            .keys()
            .filter(|k| k.name == "Unresolved review")
            .count(),
        1
    );
    assert_eq!(pr.rollup().state(), RollupState::Failing);
}

fn repo_state(name: &str, prs: &[PrChecks]) -> RepoState {
    let mut state = RepoState::new(name.parse().expect("valid repo id"));
    state.prs = prs
        .iter()
        .map(|checks| rostrum_core::PullRequest {
            number: checks.number,
            node_id: rostrum_core::NodeId(format!("PR_{}", checks.number.0)),
            title: format!("PR {}", checks.number.0),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: "h".into(),
            head_sha: checks.head_sha.clone(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Default::default(),
            merge_state: Default::default(),
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
            base_divergence: None,
            is_cross_repository: false,
            pushed_at: None,
        })
        .collect();
    state.load = LoadState::Loaded { at: Utc::now() };
    state
}

/// The whole pipeline on real data: decode, store, build the grid.
#[test]
fn captured_checks_build_a_grid() {
    let cpython = decode(CPYTHON);
    let k8s = decode(KUBERNETES);
    let repos = vec![
        repo_state("kubernetes/kubernetes", &k8s),
        repo_state("python/cpython", &cpython),
    ];
    let mut ci = CiChecks::default();
    ci.loaded(
        &"python/cpython".parse::<RepoId>().expect("id"),
        cpython.clone(),
        Utc::now(),
    );
    ci.loaded(
        &"kubernetes/kubernetes".parse::<RepoId>().expect("id"),
        k8s,
        Utc::now(),
    );

    let grid = build_grid(&repos, &FeedFilter::default(), &ci, GridFilter::default());
    assert_eq!(grid.sections().len(), 2);
    for section in grid.sections() {
        assert!(!section.columns.is_empty());
        let unique: BTreeSet<_> = section.columns.iter().collect();
        assert_eq!(unique.len(), section.columns.len(), "a column twice");
        assert!(
            section
                .rows
                .iter()
                .all(|row| row.cells.len() == section.columns.len())
        );
        assert!(section.rows.iter().all(|row| row.head_sha.len() == 7));
    }

    let attention = build_grid(
        &repos,
        &FeedFilter::default(),
        &ci,
        GridFilter {
            needs_attention: true,
        },
    );
    let cpython_rows = attention
        .sections()
        .iter()
        .find(|s| s.repo.to_string() == "python/cpython")
        .map(|s| s.rows.iter().map(|r| r.number).collect::<Vec<_>>())
        .expect("cpython has a failing PR");
    assert_eq!(cpython_rows, vec![PrNumber(157932)]);
}

#[test]
fn a_captured_failed_job_offers_its_reruns_and_times_itself() {
    let prs = decode(CPYTHON);
    let pr = prs
        .iter()
        .find(|pr| pr.number == PrNumber(157932))
        .expect("#157932");
    let failed = pr
        .entries
        .iter()
        .find(|e| {
            matches!(
                e.source,
                CheckSource::Actions {
                    job_id: 110642708500,
                    ..
                }
            )
        })
        .expect("job");
    let targets = rerun_targets(pr, failed).expect("rerunnable");
    assert_eq!(
        targets[0],
        RerunTarget::Job {
            job_id: 110642708500
        }
    );
    assert!(targets.contains(&RerunTarget::FailedJobs {
        run_id: 36944162426
    }));

    let completed = failed.completed_at.expect("finished");
    let now: DateTime<Utc> = completed + chrono::Duration::minutes(14);
    let timing = Timing::of(failed, now);
    assert_eq!(timing.label().as_deref(), Some("finished 14m ago"));
    assert!(
        timing
            .duration_label()
            .is_some_and(|d| d.starts_with("took "))
    );
}

#[test]
fn a_captured_job_log_parses_to_its_failing_step() {
    let log = parse_log(LOG, LineLimit::Full);
    assert!(!log.lines[0].text.starts_with('\u{feff}'));
    assert!(log.lines.iter().all(|l| !l.text.contains('\u{1b}')));
    assert_eq!(log.groups.len(), 4);
    let error = log.first_error.expect("the job failed");
    assert_eq!(log.lines[error].kind, LineKind::Error);
    assert_eq!(log.lines[error].text, "Process completed with exit code 2.");
    let step = &log.steps[log.failing_step.expect("a failing step")];
    assert!(step.title.contains("rt.bat"), "{}", step.title);
    let failure = log
        .lines
        .iter()
        .position(|l| {
            l.text
                .contains("test test.test_multiprocessing_spawn.test_threads failed")
        })
        .expect("the failed test is in the excerpt");
    assert!(log.in_failing_step(failure));
    assert!(!log.search("FAILURE").is_empty());
}

#[test]
fn the_ci_variables_are_the_documents() {
    let variables = json!({ "owner": "a", "name": "b", "first": 25 });
    for name in variables.as_object().expect("object").keys() {
        assert!(CI_CHECKS.contains(&format!("${name}:")), "{name}");
    }
}
