//! The phone's GitHub polling against the stand-in: batched documents, the
//! issue cadence, the budget readout and pacing, the CI grid's skip rules,
//! and every background fetch stopping while GitHub refuses for rate
//! limiting (`docs/features/poll_budget.md`).

mod support;

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use chrono::Utc;
use rostrum_config::Config;
use rostrum_ffi::{
    RostrumCore, RostrumError,
    budget::{ApiBudgetLevel, PauseReason, PollPace},
    ci::CiGridFilter,
    feed::{FeedSnapshot, RepoBody},
};
use serde_json::{Value, json};
use support::{
    Scratch,
    github::{FakeGitHub, Pr, World},
};

const REPOS: [&str; 3] = ["octo/repo", "octo/quiet", "octo/other"];

async fn core_against(fake: &FakeGitHub, scratch: &Scratch, repos: &[&str]) -> Arc<RostrumCore> {
    Config {
        repos: repos.iter().map(|repo| repo.to_string()).collect(),
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
    core
}

fn in_an_hour() -> i64 {
    Utc::now().timestamp() + 3600
}

fn world(remaining: u32) -> World {
    World {
        viewer: "me".into(),
        repos: vec![
            (
                "octo/repo".into(),
                vec![Pr::new(1, "alice"), Pr::new(2, "bob")],
            ),
            ("octo/quiet".into(), vec![]),
            ("octo/other".into(), vec![Pr::new(7, "carol")]),
        ],
        rate: Some((5000, remaining, in_an_hour())),
        ..Default::default()
    }
}

/// Feed documents (open pull requests, not the CI query).
fn feed_calls(fake: &FakeGitHub) -> Vec<Value> {
    fake.graphql_calls("pullRequests(states: OPEN")
        .into_iter()
        .filter(|body| {
            !body["query"]
                .as_str()
                .unwrap_or_default()
                .contains("contexts(first")
        })
        .collect()
}

fn section<'a>(snapshot: &'a FeedSnapshot, repo: &str) -> &'a RepoBody {
    &snapshot
        .repos
        .iter()
        .find(|section| section.repo == repo)
        .unwrap_or_else(|| panic!("no section {repo}"))
        .body
}

#[tokio::test]
async fn a_refresh_sends_batched_documents_and_reports_the_budget() {
    let fake = FakeGitHub::start(world(4200)).await;
    let scratch = Scratch::new("budget-batches");
    let core = core_against(&fake, &scratch, &REPOS).await;

    // Nothing has answered yet: no window, the configured interval.
    let before = core.api_budget().await.expect("budget").expect("signed in");
    assert_eq!(before.graphql, None);
    assert_eq!(before.schedule.pace, PollPace::Normal);

    let snapshot = core.refresh_feed().await.expect("refresh");

    // Pull requests one repository per document (cheapest at 25 each),
    // issues all three in one, divergence for both repositories with pull
    // requests in one.
    let feed = feed_calls(&fake);
    assert_eq!(feed.len(), 3);
    assert!(feed.iter().all(|body| body["variables"]["o1"].is_null()));
    let issues = fake.graphql_calls("issues(states: OPEN");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["variables"]["n2"], json!("other"));
    let divergence = fake.graphql_calls("compare(headRef");
    assert_eq!(divergence.len(), 1);
    assert_eq!(divergence[0]["variables"]["n1"], json!("other"));
    assert!(matches!(
        section(&snapshot, "octo/other"),
        RepoBody::Pulls { .. }
    ));

    // The last answer's headers are on the snapshot.
    let budget = snapshot.budget.expect("budget");
    let graphql = budget.graphql.expect("graphql window");
    assert_eq!(graphql.limit, 5000);
    assert!(
        graphql.remaining < 4200 && graphql.remaining >= 4190,
        "{graphql:?}"
    );
    assert_eq!(graphql.level, ApiBudgetLevel::Healthy);
    let resets = graphql.resets_at.expect("resets");
    assert!(resets > SystemTime::now());
    assert_eq!(budget.schedule.pace, PollPace::Normal);
    assert_eq!(budget.schedule.delay_secs, 60);
    assert_eq!(budget.schedule.base_secs, 60);
}

#[tokio::test]
async fn the_timer_refreshes_issues_on_their_own_cadence() {
    let fake = FakeGitHub::start(world(4900)).await;
    let scratch = Scratch::new("budget-issues");
    let core = core_against(&fake, &scratch, &REPOS).await;

    core.poll_feed().await.expect("first poll");
    assert_eq!(fake.graphql_calls("issues(states: OPEN").len(), 1);
    assert_eq!(feed_calls(&fake).len(), 3);

    // A minute later the pull requests are due again; the issues are not.
    core.poll_feed().await.expect("second poll");
    assert_eq!(fake.graphql_calls("issues(states: OPEN").len(), 1);
    assert_eq!(feed_calls(&fake).len(), 6);

    // Pull to refresh takes everything.
    core.refresh_feed().await.expect("refresh");
    assert_eq!(fake.graphql_calls("issues(states: OPEN").len(), 2);

    // The cadence is a setting, clamped like the desktop's.
    let settings = core.set_issue_refresh_interval(5).await.expect("cadence");
    assert_eq!(settings.issue_refresh_secs, 10);
    assert_eq!(
        core.settings().await.expect("settings").issue_refresh_secs,
        10
    );
}

#[tokio::test]
async fn a_low_budget_stretches_the_foreground_interval() {
    let fake = FakeGitHub::start(world(600)).await;
    let scratch = Scratch::new("budget-low");
    let core = core_against(&fake, &scratch, &REPOS).await;
    core.refresh_feed().await.expect("refresh");

    let schedule = core.poll_schedule().await.expect("schedule");
    assert_eq!(
        schedule.pace,
        PollPace::Stretched {
            level: ApiBudgetLevel::Low
        }
    );
    assert!(schedule.delay_secs >= 120, "{schedule:?}");
    assert!(schedule.delay_secs <= 3600, "{schedule:?}");
    let budget = core.api_budget().await.expect("budget").expect("signed in");
    assert_eq!(
        budget.graphql.map(|window| window.level),
        Some(ApiBudgetLevel::Low)
    );
}

#[tokio::test]
async fn a_rate_limit_pause_stops_every_background_fetch() {
    let fake = FakeGitHub::start(world(4800)).await;
    let scratch = Scratch::new("budget-pause");
    let core = core_against(&fake, &scratch, &REPOS).await;
    core.refresh_feed().await.expect("refresh");
    core.check_notifications().await.expect("baseline");

    // GitHub starts refusing. The refresh that hears it keeps what every
    // repository showed rather than marking them failed.
    fake.edit(|world| world.secondary_limit = Some(60));
    let refused = core.poll_feed().await.expect("a refusal is not a failure");
    assert!(matches!(
        section(&refused, "octo/repo"),
        RepoBody::Pulls { .. }
    ));
    let schedule = core.poll_schedule().await.expect("schedule");
    let PollPace::Paused { until, reason } = schedule.pace else {
        panic!("expected a pause, got {schedule:?}");
    };
    assert_eq!(reason, PauseReason::SecondaryLimit);
    let wait = until
        .duration_since(SystemTime::now())
        .expect("in the future");
    assert!(
        wait >= Duration::from_secs(60) && wait <= Duration::from_secs(91),
        "{wait:?}"
    );
    assert!(
        schedule.delay_secs >= 60 && schedule.delay_secs <= 91,
        "{schedule:?}"
    );
    assert_eq!(
        refused
            .budget
            .as_ref()
            .and_then(|budget| budget.schedule.paused_until()),
        Some(until),
        "the snapshot says until when"
    );

    // From now on nothing is sent: not the timer, not the notification
    // check, not the grid's own fetches; what the user asks for says why.
    let sent = fake.log.requests().len();
    let polled = core.poll_feed().await.expect("timer");
    assert_eq!(
        polled
            .budget
            .as_ref()
            .and_then(|budget| budget.schedule.paused_until()),
        Some(until)
    );
    assert!(matches!(
        core.refresh_feed().await,
        Err(RostrumError::GitHubRateLimited { resets_at }) if resets_at == until
    ));
    assert!(matches!(
        core.refresh_repo("octo/repo".into()).await,
        Err(RostrumError::GitHubRateLimited { .. })
    ));
    assert!(matches!(
        core.check_notifications().await,
        Err(RostrumError::GitHubRateLimited { .. })
    ));
    let filter = CiGridFilter {
        needs_attention: false,
    };
    core.open_ci(filter, None).await.expect("open quietly");
    core.watch_ci(filter, None).await.expect("watch quietly");
    assert!(matches!(
        core.refresh_ci(filter).await,
        Err(RostrumError::GitHubRateLimited { .. })
    ));
    assert_eq!(fake.log.requests().len(), sent, "nothing sent while paused");
}

/// An Actions job of workflow "CI", `status` with no conclusion when not
/// completed.
fn job(id: u64, status: &str, conclusion: Option<&str>) -> Value {
    let ago = |minutes: i64| (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339();
    json!({
        "__typename": "CheckRun",
        "databaseId": id,
        "name": format!("job{id}"),
        "status": status,
        "conclusion": conclusion,
        "startedAt": ago(5),
        "completedAt": conclusion.map(|_| ago(1)),
        "detailsUrl": format!("https://github.com/octo/repo/actions/runs/1/job/{id}"),
        "checkSuite": {
            "databaseId": 901,
            "app": {"name": "GitHub Actions", "slug": "github-actions"},
            "workflowRun": {"databaseId": 1, "runNumber": 7, "runAttempt": 1, "workflow": {"name": "CI"}}
        }
    })
}

/// A pull request whose head is the one the fake's CI answer reports.
fn pr(number: u32) -> Pr {
    let mut pr = Pr::new(number, "alice");
    pr.head_sha = format!("sha{number}aaaaaaa");
    pr
}

#[tokio::test]
async fn the_grid_fetches_only_the_checks_that_could_have_changed() {
    let fake = FakeGitHub::start(World {
        viewer: "me".into(),
        repos: vec![("octo/repo".into(), vec![pr(1), pr(2)])],
        ci: vec![
            (1, vec![job(11, "COMPLETED", Some("SUCCESS"))]),
            (2, vec![job(12, "COMPLETED", Some("FAILURE"))]),
        ],
        ..Default::default()
    })
    .await;
    let scratch = Scratch::new("budget-ci");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    let filter = CiGridFilter {
        needs_attention: false,
    };
    let checks = |fake: &FakeGitHub| fake.graphql_calls("contexts(first").len();

    // The feed never fetches checks.
    core.refresh_feed().await.expect("feed");
    assert_eq!(checks(&fake), 0);

    // Opening: never fetched, so fetched.
    let grid = core.open_ci(filter, None).await.expect("open");
    assert_eq!(grid.rows.len(), 2);
    assert_eq!(checks(&fake), 1);

    // Opening again with every head where it was and nothing running:
    // the held checks stand, and the watch has nothing to watch.
    core.open_ci(filter, None).await.expect("reopen");
    core.watch_ci(filter, None).await.expect("watch");
    assert_eq!(checks(&fake), 1);

    // A head moves: the next opening fetches.
    fake.edit(|world| world.repos[0].1[1].head_sha = "moved".into());
    core.poll_feed().await.expect("feed");
    core.open_ci(filter, None).await.expect("moved");
    assert_eq!(checks(&fake), 2);

    // Back where the checks say, and a re-run started from the web: the
    // heads did not move, so opening reuses what is held; pull to refresh
    // sees it.
    fake.edit(|world| {
        world.repos[0].1[1].head_sha = "sha2aaaaaaa".into();
        world.ci[1].1 = vec![job(13, "IN_PROGRESS", None)];
    });
    core.poll_feed().await.expect("feed");
    core.open_ci(filter, None).await.expect("unchanged");
    assert_eq!(checks(&fake), 2);
    let grid = core.refresh_ci(filter).await.expect("refresh");
    assert!(grid.any_running);
    assert_eq!(checks(&fake), 3);

    // While something runs, the watch fetches it, until it finishes.
    core.watch_ci(filter, Some("octo/repo".into()))
        .await
        .expect("watch");
    assert_eq!(checks(&fake), 4);
    fake.edit(|world| world.ci[1].1 = vec![job(13, "COMPLETED", Some("SUCCESS"))]);
    let done = core.watch_ci(filter, None).await.expect("watch");
    assert!(!done.any_running);
    assert_eq!(checks(&fake), 5);
    core.watch_ci(filter, None).await.expect("idle watch");
    assert_eq!(checks(&fake), 5);
}

#[tokio::test]
async fn the_notification_check_reads_pull_requests_in_batches_only() {
    let fake = FakeGitHub::start(world(4900)).await;
    let scratch = Scratch::new("budget-notify");
    let core = core_against(&fake, &scratch, &REPOS).await;
    core.check_notifications().await.expect("check");
    assert_eq!(feed_calls(&fake).len(), 3);
    assert!(fake.graphql_calls("issues(states: OPEN").is_empty());
    assert!(fake.graphql_calls("compare(headRef").is_empty());
}

#[tokio::test]
async fn one_probe_re_checks_every_repository_still_computing() {
    let computing = |number| {
        let mut pr = Pr::new(number, "alice");
        pr.mergeable = "UNKNOWN";
        pr.merge_state = "UNKNOWN";
        pr
    };
    let fake = FakeGitHub::start(World {
        viewer: "me".into(),
        repos: vec![
            ("octo/repo".into(), vec![computing(1)]),
            ("octo/other".into(), vec![computing(7)]),
        ],
        ..Default::default()
    })
    .await;
    let scratch = Scratch::new("budget-probe");
    let core = core_against(&fake, &scratch, &["octo/repo", "octo/other"]).await;
    let first = core.refresh_feed().await.expect("refresh");
    assert!(first.merge_states_settling);
    assert_eq!(feed_calls(&fake).len(), 2);

    // GitHub finishes computing; the first probe (2 s later) asks again for
    // both repositories at once, and nothing else is probed.
    fake.edit(|world| {
        for (_, prs) in &mut world.repos {
            prs[0].mergeable = "MERGEABLE";
            prs[0].merge_state = "CLEAN";
        }
    });
    let mut settled = false;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let snapshot = core.cached_feed().await.expect("feed");
        if !snapshot.merge_states_settling && feed_calls(&fake).len() == 4 {
            settled = true;
            break;
        }
    }
    assert!(settled, "both repositories were probed and settled");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(feed_calls(&fake).len(), 4);
    // The probe re-queries pull requests only.
    assert_eq!(fake.graphql_calls("compare(headRef").len(), 1);
}
