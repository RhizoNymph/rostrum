//! Editing an issue (with conflict detection and overwrite) and loading
//! earlier pages of issue and pull request conversations, end to end against
//! the GitHub stand-in.

mod support;

use std::{sync::Arc, time::Duration};

use rostrum_config::Config;
use rostrum_ffi::{
    RostrumCore, RostrumError,
    detail::{TimelineEntry, TimelineKind},
};
use serde_json::{Value, json};
use support::{
    Scratch,
    github::{EDITED_AT, FakeGitHub, Iss, Pr, World},
};

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
    core.refresh_feed().await.expect("refresh");
    core
}

fn world(paged: bool) -> World {
    World {
        viewer: "me".into(),
        repos: vec![("octo/repo".into(), vec![Pr::new(1, "alice")])],
        issues: vec![Iss::new(10, "alice")],
        paged,
        ..Default::default()
    }
}

/// The ids of the comments in a timeline, in order.
fn comments(timeline: &[TimelineEntry]) -> Vec<String> {
    timeline
        .iter()
        .filter(|entry| matches!(entry.kind, TimelineKind::Comment { .. }))
        .map(|entry| entry.id.clone())
        .collect()
}

fn description(timeline: &[TimelineEntry]) -> String {
    timeline
        .iter()
        .find_map(|entry| match &entry.kind {
            TimelineKind::Description { source, .. } => Some(source.clone()),
            _ => None,
        })
        .expect("a description")
}

/// GraphQL requests containing `needle` that paged back from `before`.
fn earlier_calls(fake: &FakeGitHub, needle: &str) -> Vec<Value> {
    fake.graphql_calls(needle)
        .into_iter()
        .filter(|body| body["variables"]["commentsBefore"] == "older")
        .collect()
}

#[tokio::test]
async fn an_issue_loads_earlier_comments_and_keeps_them() {
    let fake = FakeGitHub::start(world(true)).await;
    let scratch = Scratch::new("issue-paging");
    let core = core_against(&fake, &scratch).await;

    let first = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("detail");
    assert!(first.has_earlier);
    assert_eq!(first.earlier_count, Some(2));
    assert_eq!(comments(&first.timeline), vec!["IC_9"]);

    let merged = core
        .load_earlier_issue("octo/repo".into(), 10)
        .await
        .expect("earlier");
    assert!(!merged.has_earlier);
    assert_eq!(merged.earlier_count, None);
    assert_eq!(
        comments(&merged.timeline),
        vec!["IC_OLD1", "IC_OLD2", "IC_9"]
    );
    // Only the connection with more behind it was asked for.
    let calls = earlier_calls(&fake, "issue(number:");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["variables"]["withComments"], true);
    assert_eq!(calls[0]["variables"]["withEvents"], false);

    // Nothing more to load: no request, the same detail.
    let again = core
        .load_earlier_issue("octo/repo".into(), 10)
        .await
        .expect("again");
    assert_eq!(again, merged);
    assert_eq!(earlier_calls(&fake, "issue(number:").len(), 1);

    // A reload is the newest page; the earlier comments stay.
    let reloaded = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("reload");
    assert_eq!(
        comments(&reloaded.timeline),
        vec!["IC_OLD1", "IC_OLD2", "IC_9"]
    );
    drop(core);

    // The cache keeps the loaded pages.
    let reopened = RostrumCore::open(scratch.path()).await.expect("reopen");
    let mut cached = None;
    for _ in 0..100 {
        cached = reopened
            .cached_issue_detail("octo/repo".into(), 10)
            .await
            .expect("cached")
            .filter(|detail| comments(&detail.timeline).len() == 3);
        if cached.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let cached = cached.expect("the merged pages in the cache");
    assert!(!cached.has_earlier);
}

#[tokio::test]
async fn a_pull_request_loads_earlier_comments_and_keeps_them() {
    let fake = FakeGitHub::start(world(true)).await;
    let scratch = Scratch::new("pull-paging");
    let core = core_against(&fake, &scratch).await;

    let first = core
        .pull_detail("octo/repo".into(), 1)
        .await
        .expect("detail");
    assert!(first.has_earlier);
    assert_eq!(first.earlier_count, Some(2));
    assert_eq!(comments(&first.timeline), vec!["IC_1"]);
    assert_eq!(first.threads.len(), 1);

    let merged = core
        .load_earlier_pull("octo/repo".into(), 1)
        .await
        .expect("earlier");
    assert!(!merged.has_earlier);
    assert_eq!(merged.earlier_count, None);
    assert_eq!(
        comments(&merged.timeline),
        vec!["IC_OLD1", "IC_OLD2", "IC_1"]
    );
    // The threads were complete: not asked for again, still held once.
    let calls = earlier_calls(&fake, "reviewThreads");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["variables"]["withThreads"], false);
    assert_eq!(merged.threads.len(), 1);

    let reloaded = core
        .pull_detail("octo/repo".into(), 1)
        .await
        .expect("reload");
    assert_eq!(
        comments(&reloaded.timeline),
        vec!["IC_OLD1", "IC_OLD2", "IC_1"]
    );
    let cached = core
        .cached_pull_detail("octo/repo".into(), 1)
        .await
        .expect("cached")
        .expect("held");
    assert_eq!(comments(&cached.timeline).len(), 3);
}

#[tokio::test]
async fn an_unpaged_conversation_has_nothing_earlier() {
    let fake = FakeGitHub::start(world(false)).await;
    let scratch = Scratch::new("unpaged");
    let core = core_against(&fake, &scratch).await;
    let issue = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("issue");
    assert!(!issue.has_earlier);
    assert_eq!(issue.earlier_count, None);
    let pull = core.pull_detail("octo/repo".into(), 1).await.expect("pull");
    assert!(!pull.has_earlier);
    // Loading earlier on a complete conversation sends nothing.
    let requests = fake.log.requests().len();
    core.load_earlier_pull("octo/repo".into(), 1)
        .await
        .expect("pull earlier");
    core.load_earlier_issue("octo/repo".into(), 10)
        .await
        .expect("issue earlier");
    assert_eq!(fake.log.requests().len(), requests);
}

#[tokio::test]
async fn loading_earlier_before_opening_is_invalid() {
    let fake = FakeGitHub::start(world(true)).await;
    let scratch = Scratch::new("earlier-unopened");
    let core = core_against(&fake, &scratch).await;
    assert!(matches!(
        core.load_earlier_issue("octo/repo".into(), 10).await,
        Err(RostrumError::InvalidInput { .. })
    ));
}

fn patches(fake: &FakeGitHub) -> Vec<Value> {
    fake.rest_calls("PATCH", "/repos/octo/repo/issues/10")
        .iter()
        .map(|call| serde_json::from_str(&call.body).expect("json"))
        .collect()
}

#[tokio::test]
async fn an_edit_with_no_concurrent_change_is_saved_and_shown() {
    let fake = FakeGitHub::start(world(false)).await;
    let scratch = Scratch::new("edit-clear");
    let core = core_against(&fake, &scratch).await;
    let opened = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("detail");

    let saved = core
        .edit_issue(
            "octo/repo".into(),
            10,
            "  Crash on start ".into(),
            "It crashes on start.".into(),
            opened.issue.updated_at,
            false,
        )
        .await
        .expect("edit");
    assert_eq!(
        patches(&fake),
        vec![json!({"title": "Crash on start", "body": "It crashes on start."})]
    );
    assert_eq!(saved.issue.title, "Crash on start");
    assert_eq!(description(&saved.timeline), "It crashes on start.");
    // The feed's row follows.
    let snapshot = core
        .set_feed_tab(rostrum_ffi::feed::FeedTab::Issues)
        .await
        .expect("tab");
    let rostrum_ffi::feed::RepoBody::Issues { issues } = &snapshot.repos[0].body else {
        panic!("issues");
    };
    assert_eq!(issues[0].title, "Crash on start");
}

#[tokio::test]
async fn activity_that_does_not_touch_the_text_is_not_a_conflict() {
    let fake = FakeGitHub::start(world(false)).await;
    let scratch = Scratch::new("edit-activity");
    let core = core_against(&fake, &scratch).await;
    let opened = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("detail");
    // A comment elsewhere moves `updatedAt` only.
    fake.edit(|world| world.issues[0].updated_at = "2026-02-01T00:00:00Z".into());
    core.edit_issue(
        "octo/repo".into(),
        10,
        "Renamed".into(),
        "It **breaks**.".into(),
        opened.issue.updated_at,
        false,
    )
    .await
    .expect("edit");
    assert_eq!(patches(&fake).len(), 1);
}

#[tokio::test]
async fn a_concurrent_edit_is_reported_and_can_be_overwritten() {
    let fake = FakeGitHub::start(world(false)).await;
    let scratch = Scratch::new("edit-conflict");
    let core = core_against(&fake, &scratch).await;
    let opened = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("detail");
    fake.edit(|world| {
        let issue = &mut world.issues[0];
        issue.title = "Their title".into();
        issue.body = "Their body".into();
        issue.updated_at = "2026-02-01T00:00:00Z".into();
    });

    let conflict = core
        .edit_issue(
            "octo/repo".into(),
            10,
            "My title".into(),
            "My body".into(),
            opened.issue.updated_at,
            false,
        )
        .await;
    let Err(RostrumError::EditConflict {
        title,
        body,
        updated_at,
    }) = conflict
    else {
        panic!("expected a conflict, got {conflict:?}");
    };
    assert_eq!(title, "Their title");
    assert_eq!(body, "Their body");
    assert_eq!(
        updated_at,
        std::time::SystemTime::from(
            chrono::DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z").expect("time")
        )
    );
    assert!(patches(&fake).is_empty(), "nothing was sent");

    let saved = core
        .edit_issue(
            "octo/repo".into(),
            10,
            "My title".into(),
            "My body".into(),
            opened.issue.updated_at,
            true,
        )
        .await
        .expect("overwrite");
    assert_eq!(
        patches(&fake),
        vec![json!({"title": "My title", "body": "My body"})]
    );
    assert_eq!(saved.issue.title, "My title");
    assert_eq!(
        saved.issue.updated_at,
        std::time::SystemTime::from(chrono::DateTime::parse_from_rfc3339(EDITED_AT).expect("time"))
    );

    // Saving from the newer baseline is clear again.
    core.edit_issue(
        "octo/repo".into(),
        10,
        "My title, again".into(),
        "My body".into(),
        saved.issue.updated_at,
        false,
    )
    .await
    .expect("edit from the new baseline");
    assert_eq!(patches(&fake).len(), 2);
}

#[tokio::test]
async fn invalid_and_unchanged_edits_send_nothing() {
    let fake = FakeGitHub::start(world(false)).await;
    let scratch = Scratch::new("edit-invalid");
    let core = core_against(&fake, &scratch).await;
    let opened = core
        .issue_detail("octo/repo".into(), 10)
        .await
        .expect("detail");

    let blank = core
        .edit_issue(
            "octo/repo".into(),
            10,
            "   ".into(),
            "body".into(),
            opened.issue.updated_at,
            false,
        )
        .await;
    assert!(
        matches!(blank, Err(RostrumError::InvalidInput { .. })),
        "{blank:?}"
    );
    let blank_overwrite = core
        .edit_issue(
            "octo/repo".into(),
            10,
            " ".into(),
            "body".into(),
            opened.issue.updated_at,
            true,
        )
        .await;
    assert!(matches!(
        blank_overwrite,
        Err(RostrumError::InvalidInput { .. })
    ));

    let unchanged = core
        .edit_issue(
            "octo/repo".into(),
            10,
            "Issue 10".into(),
            "It **breaks**.".into(),
            opened.issue.updated_at,
            false,
        )
        .await
        .expect("unchanged");
    assert_eq!(unchanged.issue.title, "Issue 10");
    assert!(patches(&fake).is_empty());
}
