//! The GitHub half of the core, end to end, against a local stand-in for
//! api.github.com: the refresh pipeline (parallel fetch, divergence batch,
//! per-repository failure, cache), background merge-state re-checks, the
//! notification check, and what every mutation actually sends.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use rostrum_config::Config;
use rostrum_ffi::{
    RostrumCore, RostrumError,
    detail::{BranchUpdateMethod, MergeMethod, TimelineKind},
    diff::{DiffRow, FileDiffBody, LineKind},
    feed::{FeedObserver, FeedSnapshot, RepoBody},
    notifications::NotificationKind,
    review::ReviewEvent,
    session::GitHubStatus,
    types::{ColorRole, MergeStatus, PullState},
};
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

fn world(prs: Vec<Pr>) -> World {
    World {
        viewer: "me".into(),
        repos: vec![("octo/repo".into(), prs)],
        ..Default::default()
    }
}

fn section<'a>(snapshot: &'a FeedSnapshot, repo: &str) -> &'a RepoBody {
    &snapshot
        .repos
        .iter()
        .find(|section| section.repo == repo)
        .unwrap_or_else(|| panic!("no section {repo}"))
        .body
}

fn pulls(snapshot: &FeedSnapshot) -> Vec<rostrum_ffi::feed::PrSummary> {
    match section(snapshot, "octo/repo") {
        RepoBody::Pulls { pulls } => pulls.clone(),
        other => panic!("expected pulls, got {other:?}"),
    }
}

#[tokio::test]
async fn a_refresh_fetches_every_repository_and_survives_one_failing() {
    let mut behind = Pr::new(1, "alice");
    behind.divergence = (0, 3);
    let mut mine = Pr::new(2, "Me");
    mine.review_requests = vec!["me".into()];
    let fake = FakeGitHub::start(World {
        viewer: "me".into(),
        repos: vec![
            ("octo/repo".into(), vec![behind, mine]),
            ("octo/quiet".into(), vec![]),
        ],
        ..Default::default()
    })
    .await;
    let scratch = Scratch::new("refresh");
    let core = core_against(&fake, &scratch, &["octo/repo", "octo/quiet", "octo/gone"]).await;

    let snapshot = core.refresh_feed().await.expect("refresh");
    let prs = pulls(&snapshot);
    assert_eq!(
        prs.iter().map(|pr| pr.number).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        prs[0].behind_chip.as_ref().map(|chip| chip.text.as_str()),
        Some("↓3")
    );
    assert!(
        prs[0]
            .base_divergence
            .as_ref()
            .expect("divergence")
            .fast_forwards
    );
    assert_eq!(prs[0].checks_role, ColorRole::Success);
    assert_eq!(prs[0].labels[0].color, Some(0xFFD7_3A4A));
    assert!(prs[1].is_yours && prs[1].review_requested);
    // The quiet repository is hidden; the missing one shows its error.
    assert_eq!(snapshot.hidden_empty_repos, 1);
    let RepoBody::Failed { reason } = section(&snapshot, "octo/gone") else {
        panic!("expected the missing repository to fail");
    };
    assert!(reason.contains("not found"), "{reason}");
    assert_eq!(
        snapshot.viewer.as_ref().map(|viewer| viewer.login.as_str()),
        Some("me")
    );
    assert!(matches!(
        core.github_status().await,
        GitHubStatus::Verified { .. }
    ));
    // One divergence batch, for the one repository with pull requests.
    assert_eq!(fake.graphql_calls("compare(headRef").len(), 1);
    assert_eq!(core.viewer().await.expect("viewer").login, "me");
    drop(core);

    // The next launch paints from the cache, divergence included.
    let reopened = RostrumCore::open(scratch.path()).await.expect("reopen");
    let cached = reopened.cached_feed().await.expect("cached");
    assert_eq!(
        pulls(&cached)[0]
            .base_divergence
            .as_ref()
            .map(|divergence| divergence.behind),
        Some(3)
    );
}

#[tokio::test]
async fn a_rejected_token_fails_the_refresh_and_marks_the_session() {
    let fake = FakeGitHub::start(world(vec![Pr::new(1, "alice")])).await;
    fake.edit(|world| world.reject_token = true);
    let scratch = Scratch::new("rejected");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;

    assert!(matches!(
        core.refresh_feed().await,
        Err(RostrumError::GitHubAuthFailed { .. })
    ));
    assert!(matches!(
        core.github_status().await,
        GitHubStatus::Invalid { .. }
    ));
    assert!(matches!(
        core.viewer().await,
        Err(RostrumError::GitHubAuthFailed { .. })
    ));

    // A new token is checked afresh.
    fake.edit(|world| world.reject_token = false);
    assert_eq!(
        core.set_github_token(Some("ghp_new".into()))
            .await
            .expect("token"),
        GitHubStatus::Unverified
    );
    assert_eq!(core.viewer().await.expect("viewer").login, "me");
}

#[derive(Default)]
struct Snapshots(Mutex<Vec<FeedSnapshot>>);

impl FeedObserver for Snapshots {
    fn feed_changed(&self, snapshot: FeedSnapshot) {
        if let Ok(mut all) = self.0.lock() {
            all.push(snapshot);
        }
    }
}

#[tokio::test]
async fn merge_states_github_is_computing_are_rechecked_in_the_background() {
    let mut computing = Pr::new(1, "alice");
    computing.mergeable = "UNKNOWN";
    computing.merge_state = "UNKNOWN";
    let fake = FakeGitHub::start(world(vec![computing])).await;
    let scratch = Scratch::new("probe");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    let observer = Arc::new(Snapshots::default());
    core.set_feed_observer(Some(observer.clone()))
        .await
        .expect("observe");

    let first = core.refresh_feed().await.expect("refresh");
    assert_eq!(pulls(&first)[0].merge_status, MergeStatus::Computing);
    assert!(first.merge_states_settling);

    // GitHub finishes computing before the first re-check (2s later).
    fake.edit(|world| {
        let pr = &mut world.repos[0].1[0];
        pr.mergeable = "MERGEABLE";
        pr.merge_state = "CLEAN";
    });
    let mut settled = None;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        settled = observer
            .0
            .lock()
            .expect("lock")
            .iter()
            .rev()
            .find(|snapshot| {
                !snapshot.merge_states_settling
                    && snapshot.revision > first.revision
                    && matches!(section(snapshot, "octo/repo"), RepoBody::Pulls { .. })
            })
            .cloned();
        if settled.is_some() {
            break;
        }
    }
    let settled = settled.expect("a settled snapshot arrived through the observer");
    assert_eq!(pulls(&settled)[0].merge_status, MergeStatus::Ready);
    assert_eq!(fake.graphql_calls("pullRequests(states: OPEN").len(), 2);
}

#[tokio::test]
async fn notifications_report_what_is_new_since_the_last_check() {
    let fake = FakeGitHub::start(world(vec![Pr::new(1, "alice")])).await;
    let scratch = Scratch::new("notify");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.set_notifications(true, true).await.expect("settings");

    // The first check is the baseline.
    assert!(core.check_notifications().await.expect("check").is_empty());

    fake.edit(|world| {
        let prs = &mut world.repos[0].1;
        prs[0].review_requests = vec!["me".into()];
        prs.push(Pr::new(2, "bob"));
        prs.push(Pr::new(3, "me"));
    });
    let events = core.check_notifications().await.expect("check");
    assert_eq!(
        events
            .iter()
            .map(|event| (event.number, event.kind))
            .collect::<Vec<_>>(),
        vec![
            (1, NotificationKind::ReviewRequested),
            (2, NotificationKind::NewPullRequest),
        ]
    );
    assert_eq!(events[1].author.as_deref(), Some("bob"));
    assert_eq!(events[1].url, "https://github.com/octo/repo/pull/2");
    assert!(core.check_notifications().await.expect("check").is_empty());
    drop(core);

    // The seen set survives the process: the WorkManager job runs cold.
    let cold = core_against(&fake, &scratch, &["octo/repo"]).await;
    cold.set_notifications(true, true).await.expect("settings");
    assert!(cold.check_notifications().await.expect("check").is_empty());
    fake.edit(|world| world.repos[0].1.push(Pr::new(4, "carol")));
    // Looking at the feed counts as seeing it.
    cold.refresh_feed().await.expect("refresh");
    cold.mark_notifications_seen().await.expect("seen");
    assert!(cold.check_notifications().await.expect("check").is_empty());
}

fn body_of(request: &support::Request) -> Value {
    serde_json::from_str(&request.body).expect("json body")
}

#[tokio::test]
async fn detail_diff_review_and_mutations_send_what_github_expects() {
    let fake = FakeGitHub::start(world(vec![Pr::new(1, "alice")])).await;
    let scratch = Scratch::new("mutations");
    let core = core_against(&fake, &scratch, &["octo/repo"]).await;
    core.refresh_feed().await.expect("refresh");
    let repo = || "octo/repo".to_string();

    let detail = core.pull_detail(repo(), 1).await.expect("detail");
    assert_eq!(detail.header.state, PullState::Open);
    assert_eq!(detail.header.head_sha, "sha1");
    assert!(matches!(
        detail.timeline[0].kind,
        TimelineKind::Description { .. }
    ));
    assert!(matches!(
        detail.timeline[1].kind,
        TimelineKind::Comment { .. }
    ));
    assert_eq!(detail.threads[0].id, "RT_1");
    assert!(detail.threads[0].can_reply);
    assert_eq!(detail.checks[0].status_text, "success");

    // Files are fetched once per head, then served from memory and disk.
    let overview = core.files_overview(repo(), 1).await.expect("overview");
    assert_eq!(overview.files[0].threads, 1);
    core.files_overview(repo(), 1).await.expect("overview");
    let diff = core.file_diff(repo(), 1, 0).await.expect("diff");
    assert_eq!(
        fake.rest_calls("GET", "/repos/octo/repo/pulls/1/files?per_page=100")
            .len(),
        1
    );
    let FileDiffBody::Rows { rows } = diff.body else {
        panic!("rows");
    };
    let added = rows
        .iter()
        .find_map(|row| match row {
            DiffRow::Line { line } if line.kind == LineKind::Added && line.new_line == Some(11) => {
                line.anchor.clone()
            }
            _ => None,
        })
        .expect("added line");

    // A review with an inline comment.
    core.add_draft(repo(), 1, added, None, "Why two?".into())
        .await
        .expect("draft");
    core.submit_review(
        repo(),
        1,
        ReviewEvent::RequestChanges,
        " Please fix ".into(),
        true,
    )
    .await
    .expect("review");
    let review = body_of(
        fake.rest_calls("POST", "/repos/octo/repo/pulls/1/reviews")
            .last()
            .expect("review request"),
    );
    assert_eq!(
        review,
        json!({
            "event": "REQUEST_CHANGES",
            "body": "Please fix",
            "comments": [{"path": "src/lib.rs", "line": 11, "side": "RIGHT", "body": "Why two?"}]
        })
    );
    assert!(
        core.pending_review(repo(), 1)
            .await
            .expect("pending")
            .drafts
            .is_empty()
    );
    core.submit_review(repo(), 1, ReviewEvent::Approve, String::new(), false)
        .await
        .expect("empty approval");
    assert_eq!(
        body_of(
            fake.rest_calls("POST", "/repos/octo/repo/pulls/1/reviews")
                .last()
                .expect("approval")
        )["event"],
        "APPROVE"
    );
    let reviews_before = fake
        .rest_calls("POST", "/repos/octo/repo/pulls/1/reviews")
        .len();
    assert!(matches!(
        core.submit_review(repo(), 1, ReviewEvent::Comment, " ".into(), true)
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert_eq!(
        fake.rest_calls("POST", "/repos/octo/repo/pulls/1/reviews")
            .len(),
        reviews_before
    );

    // Conversation replies and comments.
    core.reply_to_thread(repo(), 1, "RT_1".into(), "because".into())
        .await
        .expect("reply");
    assert_eq!(
        body_of(&fake.rest_calls("POST", "/repos/octo/repo/pulls/1/comments/555/replies")[0]),
        json!({"body": "because"})
    );
    core.add_comment(repo(), 1, "Thanks!".into())
        .await
        .expect("comment");
    assert_eq!(
        body_of(&fake.rest_calls("POST", "/repos/octo/repo/issues/1/comments")[0]),
        json!({"body": "Thanks!"})
    );

    // Labels: the palette once, and a name that needs encoding in a path.
    assert_eq!(
        core.repository_labels(repo()).await.expect("labels").len(),
        2
    );
    core.repository_labels(repo()).await.expect("labels");
    assert_eq!(
        fake.rest_calls("GET", "/repos/octo/repo/labels?per_page=100")
            .len(),
        1
    );
    core.add_label(repo(), 1, "help wanted".into())
        .await
        .expect("add label");
    assert_eq!(
        body_of(&fake.rest_calls("POST", "/repos/octo/repo/issues/1/labels")[0]),
        json!({"labels": ["help wanted"]})
    );
    core.remove_label(repo(), 1, "help wanted".into())
        .await
        .expect("remove label");
    assert_eq!(
        fake.rest_calls("DELETE", "/repos/octo/repo/issues/1/labels/help%20wanted")
            .len(),
        1
    );

    // Merge carries the head it was confirmed against.
    core.merge(
        repo(),
        1,
        MergeMethod::Squash,
        Some("Squashed".into()),
        None,
        "sha1".into(),
    )
    .await
    .expect("merge");
    assert_eq!(
        body_of(&fake.rest_calls("PUT", "/repos/octo/repo/pulls/1/merge")[0]),
        json!({"merge_method": "squash", "sha": "sha1", "commit_title": "Squashed"})
    );
    fake.edit(|world| world.refuse_merge = Some("Head branch was modified.".into()));
    assert_eq!(
        core.merge(repo(), 1, MergeMethod::Merge, None, None, "stale".into())
            .await,
        Err(RostrumError::MergeBlocked {
            reason: "Head branch was modified.".into()
        })
    );
    assert!(matches!(
        core.merge(repo(), 1, MergeMethod::Merge, None, None, " ".into())
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));

    // Draft conversion and branch updates are GraphQL, addressed by node id.
    core.set_draft(repo(), 1, true).await.expect("to draft");
    assert_eq!(
        fake.graphql_calls("convertPullRequestToDraft")[0]["variables"],
        json!({"id": "PR_1"})
    );
    core.set_draft(repo(), 1, false).await.expect("ready");
    assert_eq!(fake.graphql_calls("markPullRequestReadyForReview").len(), 1);
    core.update_branch(repo(), 1, BranchUpdateMethod::Rebase, "sha1".into())
        .await
        .expect("update branch");
    assert_eq!(
        fake.graphql_calls("updatePullRequestBranch")[0]["variables"],
        json!({"id": "PR_1", "oid": "sha1", "method": "REBASE"})
    );

    core.close_pull_request(repo(), 1).await.expect("close");
    core.reopen_pull_request(repo(), 1).await.expect("reopen");
    let states: Vec<Value> = fake
        .rest_calls("PATCH", "/repos/octo/repo/pulls/1")
        .iter()
        .map(body_of)
        .collect();
    assert_eq!(
        states,
        vec![json!({"state": "closed"}), json!({"state": "open"})]
    );

    // Every accepted mutation re-read the repository afterwards.
    assert!(fake.graphql_calls("pullRequests(states: OPEN").len() >= 12);
}
