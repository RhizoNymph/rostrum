//! A pull request the feed does not hold — what a `rostrum://pr` link from
//! another app names — is looked up on GitHub by itself, remembered, and then
//! behaves like one from the feed; one GitHub does not have is a 404 that
//! names it.

mod support;

use std::sync::Arc;

use rostrum_config::Config;
use rostrum_ffi::{RostrumCore, RostrumError, types::PullState};
use serde_json::{Value, json};
use support::{
    Scratch,
    github::{FakeGitHub, Pr, World},
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

fn world() -> World {
    World {
        viewer: "me".into(),
        repos: vec![
            ("octo/repo".into(), vec![Pr::new(1, "alice")]),
            ("octo/elsewhere".into(), vec![Pr::new(7, "carol")]),
        ],
        ..Default::default()
    }
}

/// The single-pull-request lookups sent. The conversation query also names
/// `pullRequest(number:`, but asks for review threads.
fn lookups(fake: &FakeGitHub) -> Vec<Value> {
    fake.graphql_calls("pullRequest(number:")
        .into_iter()
        .filter(|body| {
            !body["query"]
                .as_str()
                .unwrap_or_default()
                .contains("reviewThreads")
        })
        .collect()
}

#[tokio::test]
async fn a_pull_request_outside_the_feed_is_looked_up_once_then_acts_like_one_in_it() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("linked");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");

    // One the feed holds needs no lookup.
    core.pull_detail("octo/repo".into(), 1)
        .await
        .expect("feed detail");
    assert!(lookups(&fake).is_empty());

    let detail = core
        .pull_detail("octo/elsewhere".into(), 7)
        .await
        .expect("linked detail");
    assert_eq!(detail.header.head_sha, "sha7");
    assert_eq!(detail.header.state, PullState::Open);
    let sent = lookups(&fake);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["variables"],
        json!({"owner": "octo", "name": "elsewhere", "number": 7})
    );

    // Remembered: the header and a second open come without a lookup.
    let header = core
        .pull_header("octo/elsewhere".into(), 7)
        .await
        .expect("header");
    assert_eq!(header.head_sha, "sha7");
    core.pull_detail("octo/elsewhere".into(), 7)
        .await
        .expect("again");
    assert_eq!(lookups(&fake).len(), 1);

    // It is not added to the feed.
    let snapshot = core.refresh_feed().await.expect("refresh");
    assert!(
        snapshot
            .repos
            .iter()
            .all(|section| section.repo != "octo/elsewhere")
    );
}

#[tokio::test]
async fn a_link_to_something_github_does_not_have_is_a_404_naming_it() {
    let fake = FakeGitHub::start(world()).await;
    let scratch = Scratch::new("linked-missing");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");

    for (repo, number) in [("octo/missing", 3), ("octo/repo", 99)] {
        let Err(RostrumError::GitHubApi { status, reason }) =
            core.pull_detail(repo.into(), number).await
        else {
            panic!("expected a 404 for {repo}#{number}");
        };
        assert_eq!(status, Some(404));
        assert!(reason.contains(&format!("{repo}#{number}")), "{reason}");
    }
    // Nothing was remembered.
    assert_eq!(
        core.pull_header("octo/missing".into(), 3).await,
        Err(RostrumError::UnknownPullRequest {
            repo: "octo/missing".into(),
            number: 3
        })
    );
}
