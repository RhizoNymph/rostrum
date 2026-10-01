//! The desktop features the phone gained in one step, end to end against the
//! GitHub stand-in: the saved sort, the Issues tab and every issue action,
//! stacks in the feed, and a repository's own screen with its branch tree.

mod support;

use std::sync::Arc;

use rostrum_config::Config;
use rostrum_ffi::{
    RostrumCore, RostrumError,
    detail::{TimelineEvent, TimelineKind},
    feed::{FeedSnapshot, FeedTab, RepoBody},
    issues::{CloseIssueAs, IssueCloseReason, IssueStatus},
    repo_view::{BranchDrift, BranchRow, TrunkDrift},
    sort::{ItemSortKey, RepoSortKey, SortDirection},
    stacks::{PullItem, StackKind},
};
use serde_json::{Value, json};
use support::{
    Scratch, flat_pulls,
    github::{FakeGitHub, Iss, Pr, World},
};

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

/// Reopen the profile once the dropped core's queued cache writes have
/// landed, judged by `ready` on the cached feed. The writer drains after the
/// core is dropped, so the first reopen can race it.
async fn reopened_when(
    scratch: &Scratch,
    ready: impl Fn(&FeedSnapshot) -> bool,
) -> (Arc<RostrumCore>, FeedSnapshot) {
    for _ in 0..100 {
        let core = RostrumCore::open(scratch.path()).await.expect("reopen");
        let cached = core.cached_feed().await.expect("cached");
        if ready(&cached) {
            return (core, cached);
        }
        drop(core);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the cache never caught up");
}

fn config(scratch: &Scratch) -> Config {
    Config::load_from(&scratch.dir.join("config.json")).0
}

/// `octo/repo`: #1 alone on main, #2 → #3 a chain (#3 based on #2's head),
/// and three issues.
fn world() -> World {
    let mut top = Pr::new(3, "alice");
    top.base = "topic-2".into();
    let mut issue = Iss::new(10, "alice");
    issue.labels = vec!["bug".into()];
    issue.assignees = vec!["me".into()];
    issue.comments = 4;
    World {
        viewer: "me".into(),
        repos: vec![(
            "octo/repo".into(),
            vec![Pr::new(1, "bob"), Pr::new(2, "alice"), top],
        )],
        issues: vec![issue, Iss::new(11, "bob"), Iss::new(12, "carol")],
        default_branch: Some("main".into()),
        branches: vec!["develop".into()],
        stars: 42,
        assignees: vec!["me".into(), "bob".into()],
        ..Default::default()
    }
}

fn items(snapshot: &FeedSnapshot) -> &[PullItem] {
    match &snapshot.repos[0].body {
        RepoBody::Pulls { items } => items,
        other => panic!("expected pulls, got {other:?}"),
    }
}

fn issue_numbers(snapshot: &FeedSnapshot) -> Vec<u32> {
    match &snapshot.repos[0].body {
        RepoBody::Issues { issues } => issues.iter().map(|issue| issue.number).collect(),
        other => panic!("expected issues, got {other:?}"),
    }
}

#[tokio::test]
async fn a_refresh_brings_issues_and_stacks_and_the_tab_persists() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("issues-tab");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;

    let snapshot = core.refresh_feed().await.expect("refresh");
    assert_eq!(snapshot.tab, FeedTab::PullRequests);
    assert_eq!(snapshot.tab_counts.pull_requests, 3);
    assert_eq!(snapshot.tab_counts.issues, 3);
    // The chain is one item: its header, then #2 and #3 bottom first.
    let items = items(&snapshot);
    assert_eq!(items.len(), 2);
    let stack = items
        .iter()
        .find_map(|item| match item {
            PullItem::Stack { stack, members } => Some((stack, members)),
            PullItem::Single { .. } => None,
        })
        .expect("a stack");
    assert_eq!(stack.0.kind, StackKind::Chain);
    assert_eq!(stack.0.title, "Stackable chain · 2 PRs");
    assert_eq!(stack.0.trunk, "main");
    assert_eq!(
        stack.1.iter().map(|pull| pull.number).collect::<Vec<_>>(),
        vec![2, 3]
    );
    // The repository has no Stacks API (404): asked once, remembered.
    assert_eq!(
        fake.rest_calls("GET", "/repos/octo/repo/stacks?per_page=100")
            .len(),
        1
    );
    core.refresh_feed().await.expect("again");
    assert_eq!(
        fake.rest_calls("GET", "/repos/octo/repo/stacks?per_page=100")
            .len(),
        1
    );

    let issues = core.set_feed_tab(FeedTab::Issues).await.expect("tab");
    assert_eq!(issues.tab, FeedTab::Issues);
    // Newest first by default.
    assert_eq!(issue_numbers(&issues), vec![12, 11, 10]);
    let RepoBody::Issues { issues: rows } = &issues.repos[0].body else {
        panic!("issues");
    };
    let mine = &rows[2];
    assert_eq!(mine.status, IssueStatus::Open);
    assert_eq!(mine.labels[0].name, "bug");
    assert_eq!(mine.labels[0].color, Some(0xFFD7_3A4A));
    assert_eq!(mine.comment_count, 4);
    assert!(mine.is_yours || mine.assigned_to_you);
    assert_eq!(mine.milestone.as_deref(), Some("v1"));
    assert_eq!(config(&scratch).feed_tab, rostrum_core::FeedTab::Issues);
    drop(core);

    // The next launch opens on the Issues tab, from the cache.
    let (_reopened, cached) = reopened_when(&scratch, |cached| {
        matches!(&cached.repos.first().map(|section| &section.body), Some(RepoBody::Issues { issues }) if issues.len() == 3)
    })
    .await;
    assert_eq!(cached.tab, FeedTab::Issues);
    assert_eq!(issue_numbers(&cached), vec![12, 11, 10]);
    assert_eq!(cached.tab_counts.pull_requests, 3);
}

#[tokio::test]
async fn a_github_stack_is_grouped_by_its_number() {
    let mut world = world();
    world.stacks = Some(json!([
        {"number": 4, "base": {"ref": "main"}, "open": true,
         "pull_requests": [{"number": 2}, {"number": 3}, {"number": 30}]}
    ]));
    let fake = FakeGitHub::start(world).await;
    let scratch = Scratch::new("github-stack");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    let snapshot = core.refresh_feed().await.expect("refresh");
    let stack = items(&snapshot)
        .iter()
        .find_map(|item| match item {
            PullItem::Stack { stack, .. } => Some(stack.clone()),
            PullItem::Single { .. } => None,
        })
        .expect("a stack");
    assert_eq!(stack.kind, StackKind::GitHub { number: 4 });
    assert_eq!(stack.title, "Stack 4 · 3 PRs");
    assert_eq!(stack.member_count, 3);
    assert_eq!(stack.absent, 1);
    assert_eq!(stack.rollup.as_ref().map(|rollup| rollup.total), Some(2));
    drop(core);
    // Cached with the pull requests.
    reopened_when(&scratch, |cached| {
        matches!(&cached.repos.first().map(|section| &section.body), Some(RepoBody::Pulls { items }) if items.iter().any(|item| matches!(
            item,
            PullItem::Stack { stack, .. } if stack.kind == StackKind::GitHub { number: 4 }
        )))
    })
    .await;
}

#[tokio::test]
async fn the_sorts_reorder_the_feed_and_persist() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("sorts");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");

    let settings = core.sort_settings().await.expect("settings");
    assert_eq!(settings.repo_key, RepoSortKey::Pushed);
    assert_eq!(settings.item_key, ItemSortKey::Created);
    assert_eq!(settings.item_direction, SortDirection::Descending);

    // Author A→Z (the key's default direction): alice's #2→#3 stack, then bob's #1.
    let by_author = core
        .set_item_sort(ItemSortKey::Author, None)
        .await
        .expect("author");
    assert_eq!(by_author.sort.item_key, ItemSortKey::Author);
    assert_eq!(by_author.sort.item_direction, SortDirection::Ascending);
    assert_eq!(
        flat_pulls(items(&by_author))
            .iter()
            .map(|pull| pull.number)
            .collect::<Vec<_>>(),
        vec![2, 3, 1]
    );
    let flipped = core
        .set_item_sort(ItemSortKey::Author, Some(SortDirection::Descending))
        .await
        .expect("flip");
    assert_eq!(
        flat_pulls(items(&flipped))
            .iter()
            .map(|pull| pull.number)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let stars = core
        .set_repo_sort(RepoSortKey::Stars, None)
        .await
        .expect("stars");
    assert_eq!(stars.sort.repo_key, RepoSortKey::Stars);
    assert_eq!(stars.sort.repo_direction_label, "Most");

    // The sort survives clearing the filter, and a relaunch.
    let cleared = core.clear_filter().await.expect("clear");
    assert_eq!(cleared.sort.item_key, ItemSortKey::Author);
    let saved = config(&scratch);
    assert_eq!(
        saved.feed_filter().sort,
        rostrum_core::FeedSort {
            repos: rostrum_core::Sort::new(rostrum_core::RepoSortKey::Stars),
            items: rostrum_core::Sort::with_direction(
                rostrum_core::ItemSortKey::Author,
                rostrum_core::SortDirection::Descending,
            ),
        }
    );
    drop(core);
    let reopened = RostrumCore::open(scratch.path()).await.expect("reopen");
    assert_eq!(
        reopened
            .sort_settings()
            .await
            .expect("settings")
            .item_direction,
        SortDirection::Descending
    );
}

#[tokio::test]
async fn the_issue_screen_shows_a_typed_timeline_and_is_cached() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("issue-detail");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");

    assert!(
        core.cached_issue_detail("octo/repo".into(), 10)
            .await
            .expect("cached")
            .is_none()
    );
    let detail = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("detail");
    assert_eq!(detail.issue.number, 10);
    assert_eq!(detail.issue.title, "Issue 10");
    assert!(detail.issue.assigned_to_you);
    let TimelineKind::Description { body, .. } = &detail.timeline[0].kind else {
        panic!("description first");
    };
    assert!(!body.is_empty());
    let events: Vec<&TimelineEvent> = detail
        .timeline
        .iter()
        .filter_map(|entry| match &entry.kind {
            TimelineKind::Event { event, .. } => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(
        events,
        vec![
            &TimelineEvent::ClosedAs {
                reason: IssueCloseReason::NotPlanned
            },
            &TimelineEvent::Reopened,
            &TimelineEvent::Unassigned {
                assignee: "dave".into()
            },
            &TimelineEvent::CrossReferenced {
                source: "octo/repo#1".into(),
                title: "Pull request 1".into()
            },
        ]
    );
    assert!(detail.timeline.iter().any(|entry| matches!(
        &entry.kind,
        TimelineKind::Comment { source, .. } if source == "Same here"
    )));
    drop(core);

    let (reopened, _) = reopened_when(&scratch, |_| true).await;
    let mut cached = None;
    for _ in 0..100 {
        cached = reopened
            .cached_issue_detail("octo/repo".into(), 10)
            .await
            .expect("cached");
        if cached.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let cached = cached.expect("in the cache");
    // Signed out, the cache cannot say whose it is; the rest is identical.
    assert_eq!(cached.timeline, detail.timeline);
    assert_eq!(cached.issue.labels, detail.issue.labels);
}

fn last_body(fake: &FakeGitHub, method: &str, path: &str) -> Value {
    let calls = fake.rest_calls(method, path);
    let call = calls.last().unwrap_or_else(|| panic!("no {method} {path}"));
    serde_json::from_str(&call.body).expect("json body")
}

#[tokio::test]
async fn every_issue_action_sends_what_github_expects_and_refreshes_the_tab() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("issue-actions");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");
    let repo = || "octo/repo".to_string();

    core.comment_on_issue(repo(), 11, "On it".into())
        .await
        .expect("comment");
    assert_eq!(
        last_body(&fake, "POST", "/repos/octo/repo/issues/11/comments"),
        json!({"body": "On it"})
    );
    assert!(matches!(
        core.comment_on_issue(repo(), 11, "  ".into()).await,
        Err(RostrumError::InvalidInput { .. })
    ));

    core.add_issue_label(repo(), 11, "help wanted".into())
        .await
        .expect("label");
    assert_eq!(
        last_body(&fake, "POST", "/repos/octo/repo/issues/11/labels"),
        json!({"labels": ["help wanted"]})
    );
    core.remove_issue_label(repo(), 11, "help wanted".into())
        .await
        .expect("unlabel");
    assert_eq!(
        fake.rest_calls("DELETE", "/repos/octo/repo/issues/11/labels/help%20wanted")
            .len(),
        1
    );

    let users = core.assignable_users(repo()).await.expect("assignable");
    assert_eq!(
        users
            .iter()
            .map(|user| user.login.as_str())
            .collect::<Vec<_>>(),
        vec!["me", "bob"]
    );
    core.assignable_users(repo()).await.expect("again");
    assert_eq!(
        fake.rest_calls("GET", "/repos/octo/repo/assignees?per_page=100")
            .len(),
        1
    );
    core.add_issue_assignee(repo(), 11, "bob".into())
        .await
        .expect("assign");
    assert_eq!(
        last_body(&fake, "POST", "/repos/octo/repo/issues/11/assignees"),
        json!({"assignees": ["bob"]})
    );
    core.remove_issue_assignee(repo(), 11, "bob".into())
        .await
        .expect("unassign");
    assert_eq!(
        last_body(&fake, "DELETE", "/repos/octo/repo/issues/11/assignees"),
        json!({"assignees": ["bob"]})
    );

    // Closing re-reads the issues: the closed one leaves the tab.
    let issue_reads = fake.graphql_calls("issues(states: OPEN").len();
    core.close_issue(repo(), 11, CloseIssueAs::NotPlanned)
        .await
        .expect("close");
    assert_eq!(
        last_body(&fake, "PATCH", "/repos/octo/repo/issues/11"),
        json!({"state": "closed", "state_reason": "not_planned"})
    );
    assert!(fake.graphql_calls("issues(states: OPEN").len() > issue_reads);
    let snapshot = core.set_feed_tab(FeedTab::Issues).await.expect("tab");
    assert_eq!(issue_numbers(&snapshot), vec![12, 10]);
    assert_eq!(snapshot.tab_counts.issues, 2);

    core.reopen_issue(repo(), 10).await.expect("reopen");
    assert_eq!(
        last_body(&fake, "PATCH", "/repos/octo/repo/issues/10"),
        json!({"state": "open", "state_reason": "reopened"})
    );
    core.close_issue(repo(), 10, CloseIssueAs::Completed)
        .await
        .expect("close completed");
    assert_eq!(
        last_body(&fake, "PATCH", "/repos/octo/repo/issues/10"),
        json!({"state": "closed", "state_reason": "completed"})
    );
}

#[tokio::test]
async fn creating_an_issue_validates_then_sends_one_request() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("issue-create");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");

    let blank = core
        .create_issue(
            "octo/repo".into(),
            "   ".into(),
            "body".into(),
            vec![],
            vec![],
        )
        .await;
    assert!(
        matches!(blank, Err(RostrumError::InvalidInput { .. })),
        "{blank:?}"
    );
    assert!(
        fake.rest_calls("POST", "/repos/octo/repo/issues")
            .is_empty()
    );

    let number = core
        .create_issue(
            "octo/repo".into(),
            "  Crash on start  ".into(),
            "Steps".into(),
            vec!["bug".into(), "bug".into()],
            vec!["me".into()],
        )
        .await
        .expect("create");
    assert_eq!(number, 99);
    let sent = last_body(&fake, "POST", "/repos/octo/repo/issues");
    assert_eq!(sent["title"], "Crash on start");
    assert_eq!(sent["body"], "Steps");
    assert_eq!(sent["labels"], json!(["bug"]));
    assert_eq!(sent["assignees"], json!(["me"]));

    // No blank body or empty lists go over the wire.
    core.create_issue(
        "octo/repo".into(),
        "Bare".into(),
        " ".into(),
        vec![],
        vec![],
    )
    .await
    .expect("bare");
    assert_eq!(
        last_body(&fake, "POST", "/repos/octo/repo/issues"),
        json!({"title": "Bare"})
    );
}

#[tokio::test]
async fn the_repository_screen_lists_everything_unfiltered_with_its_branch_tree() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("repo-screen");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");
    // A feed filter that hides everything does not narrow the screen.
    core.set_query("nothing matches this".into())
        .await
        .expect("query");

    let overview = core
        .repo_overview("octo/repo".into())
        .await
        .expect("overview");
    assert_eq!(overview.url, "https://github.com/octo/repo");
    assert_eq!(overview.default_branch, None);
    assert_eq!(
        flat_pulls(&overview.pulls)
            .iter()
            .map(|pull| pull.number)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(matches!(overview.pulls[1], PullItem::Stack { .. }));
    assert_eq!(
        overview
            .issues
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>(),
        vec![12, 11, 10]
    );
    assert!(matches!(
        core.repo_overview("octo/other".into()).await,
        Err(RostrumError::InvalidInput { .. })
    ));

    let tree = core.branch_tree("octo/repo".into()).await.expect("tree");
    assert_eq!(tree.stars, 42);
    assert_eq!(tree.default_branch.as_deref(), Some("main"));
    assert!(tree.trunks.detected);
    assert_eq!(tree.trunks.existing, vec!["develop", "main"]);
    let shape: Vec<String> = tree
        .rows
        .iter()
        .map(|row| match row {
            BranchRow::Trunk { name, drift, pulls } => format!("T {name} {drift:?} {pulls}"),
            BranchRow::OtherBases => "Other".into(),
            BranchRow::Base { name, pulls } => format!("B {name} {pulls}"),
            BranchRow::Pull {
                depth,
                number,
                stack_label,
                ..
            } => format!(
                "P{depth} #{number} {}",
                stack_label.as_deref().unwrap_or("-")
            ),
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            "T main Default 3".to_string(),
            "P1 #1 -".into(),
            "P1 #2 chain".into(),
            "P2 #3 chain".into(),
            "T develop Unknown 0".into(),
        ]
    );
    let BranchRow::Pull { drift, pull, .. } = &tree.rows[1] else {
        panic!("a pull");
    };
    assert_eq!(
        *drift,
        Some(BranchDrift {
            ahead: 1,
            behind: 0
        })
    );
    assert_eq!(pull.as_ref().map(|pull| pull.number), Some(1));
    assert!(matches!(
        tree.rows[4],
        BranchRow::Trunk {
            drift: TrunkDrift::Unknown,
            ..
        }
    ));

    // The overview now knows what the tree fetched.
    let overview = core
        .repo_overview("octo/repo".into())
        .await
        .expect("overview");
    assert_eq!(overview.stars, Some(42));
    assert_eq!(overview.default_branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn trunks_validate_persist_and_shape_the_tree() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("trunks");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");

    let initial = core.trunks("octo/repo".into()).await.expect("trunks");
    assert!(initial.detected);
    assert!(initial.existing.is_empty());

    assert!(matches!(
        core.set_trunks("octo/repo".into(), Some(vec!["bad..name".into()]))
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(config(&scratch).trunks.is_empty());

    let set = core
        .set_trunks(
            "octo/repo".into(),
            Some(vec!["release".into(), "release".into()]),
        )
        .await
        .expect("set");
    assert!(!set.detected);
    assert_eq!(set.configured, vec!["release"]);
    assert_eq!(
        config(&scratch)
            .trunk_choice(&"octo/repo".parse().expect("repo"))
            .0,
        rostrum_core::branches::TrunkChoice::Configured(vec![
            rostrum_core::branches::TrunkName::parse("release").expect("name")
        ])
    );

    // A configured trunk GitHub lacks shows as missing.
    let tree = core.branch_tree("octo/repo".into()).await.expect("tree");
    assert!(tree.rows.iter().any(|row| matches!(
        row,
        BranchRow::Trunk { name, drift: TrunkDrift::Missing, .. } if name == "release"
    )));

    let detected = core
        .set_trunks("octo/repo".into(), None)
        .await
        .expect("detect");
    assert!(detected.detected);
    assert!(config(&scratch).trunks.is_empty());
}

#[tokio::test]
async fn the_background_notification_check_reads_pull_requests_only() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("background");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.check_notifications().await.expect("check");
    assert!(!fake.graphql_calls("pullRequests(states: OPEN").is_empty());
    assert!(fake.graphql_calls("issues(states: OPEN").is_empty());
    assert!(
        fake.rest_calls("GET", "/repos/octo/repo/stacks?per_page=100")
            .is_empty()
    );
}
