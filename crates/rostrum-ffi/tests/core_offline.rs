//! The core end to end without a network: everything that must work from
//! the cache alone, and everything that must survive a restart.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use rostrum_core::{
    CheckRun, CheckState, CommentId, Conversation, PrNumber, PullState as CoreState, ReviewThread,
    Side, ThreadComment, ThreadId, TimelineItem, User,
};
use rostrum_db::Db;
use rostrum_ffi::{
    RostrumCore, RostrumError,
    diff::{CommentAnchor, DiffAvailability, DiffRow, FileDiffBody, LineKind},
    feed::{FeedObserver, FeedPreferences, FeedSnapshot, RepoBody, RepoLoad},
    review::ReviewEvent,
    session::GitHubStatus,
    types::{PullState, Side as FfiSide},
};
use support::{Scratch, assert_no_secret_on_disk, files, pull, repo, seed};

fn pulls(snapshot: &FeedSnapshot) -> Vec<u32> {
    match &snapshot.repos[0].body {
        RepoBody::Pulls { items } => support::flat_pulls(items)
            .iter()
            .map(|pull| pull.number)
            .collect(),
        other => panic!("expected pulls, got {other:?}"),
    }
}

#[tokio::test]
async fn settings_validate_persist_and_never_hold_a_secret() {
    let scratch = Scratch::new("settings");
    seed(&scratch.dir, &[], None).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");

    let settings = core.settings().await.expect("settings");
    assert_eq!(settings.repos, vec!["octo/repo"]);
    assert!(!settings.notify_new_pull_requests && !settings.notify_review_requests);

    assert_eq!(
        core.add_repo("https://github.com/rust-lang/rust.git".into())
            .await
            .expect("add"),
        "rust-lang/rust"
    );
    assert_eq!(
        core.add_repo("rust-lang/rust".into()).await,
        Err(RostrumError::DuplicateRepo {
            repo: "rust-lang/rust".into()
        })
    );
    assert!(matches!(
        core.add_repo("not a repo".into()).await,
        Err(RostrumError::InvalidRepo { .. })
    ));
    let feed = core.cached_feed().await.expect("feed");
    let settings = core.settings().await.expect("settings");
    assert_eq!(
        feed.repos
            .iter()
            .map(|section| section.repo.clone())
            .collect::<Vec<_>>(),
        settings.repos,
        "the feed follows settings order"
    );

    let settings = core.set_refresh_interval(3).await.expect("interval");
    assert_eq!(settings.refresh_interval_secs, 10);
    let settings = core.set_prs_per_repo(1000).await.expect("per repo");
    assert_eq!(settings.prs_per_repo, 100);
    core.set_notifications(true, true)
        .await
        .expect("notifications");
    core.set_autostash(true).await.expect("autostash");
    core.set_filter(FeedPreferences {
        hide_drafts: true,
        hide_empty_repos: false,
        authors: vec!["Alice".into(), " ".into()],
        include_involved: true,
    })
    .await
    .expect("filter");
    let searched = core.set_query("  needle ".into()).await.expect("query");
    assert_eq!(searched.query, "needle");
    assert_eq!(searched.preferences.authors, vec!["alice"]);

    core.set_github_token(Some("ghp_supersecret_token".into()))
        .await
        .expect("token");
    assert_eq!(core.github_status().await, GitHubStatus::Unverified);
    drop(core);

    // A second launch restores every preference, but not the search.
    let reopened = RostrumCore::open(scratch.path()).await.expect("reopen");
    let settings = reopened.settings().await.expect("settings");
    assert_eq!(settings.refresh_interval_secs, 10);
    assert_eq!(settings.prs_per_repo, 100);
    assert!(settings.notify_new_pull_requests && settings.notify_review_requests);
    assert!(settings.autostash);
    assert!(settings.feed.hide_drafts && settings.feed.include_involved);
    assert!(!settings.feed.hide_empty_repos);
    assert_eq!(settings.feed.authors, vec!["alice"]);
    let feed = reopened.cached_feed().await.expect("feed");
    assert_eq!(feed.query, "");
    // Nor the token: Kotlin hands it in again.
    assert_eq!(reopened.github_status().await, GitHubStatus::NoToken);

    assert!(
        reopened
            .remove_repo("rust-lang/rust".into())
            .await
            .expect("remove")
    );
    assert!(
        !reopened
            .remove_repo("rust-lang/rust".into())
            .await
            .expect("remove")
    );
    assert!(reopened.warnings().await.is_empty());
    drop(reopened);

    assert_no_secret_on_disk(&scratch.dir, "ghp_supersecret_token");
}

#[tokio::test]
async fn the_feed_paints_from_the_cache_and_filters() {
    let scratch = Scratch::new("feed");
    let mut draft = pull(2, "h2");
    draft.is_draft = true;
    seed(&scratch.dir, &[pull(1, "h1"), draft, pull(3, "h3")], None).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");

    let feed = core.cached_feed().await.expect("feed");
    assert_eq!(pulls(&feed), vec![1, 2, 3]);
    assert_eq!(feed.total_open, 3);
    assert_eq!(feed.repos[0].load, RepoLoad::Idle);
    assert!(!feed.filter_active);

    let feed = core
        .set_filter(FeedPreferences {
            hide_drafts: true,
            ..feed.preferences.clone()
        })
        .await
        .expect("filter");
    assert_eq!(pulls(&feed), vec![1, 3]);
    assert!(feed.filter_active);

    let feed = core.toggle_author("AUTHOR3".into()).await.expect("author");
    assert_eq!(pulls(&feed), vec![3]);
    assert_eq!(feed.visible_open, 1);
    assert_eq!(feed.repos[0].open_count, 3);

    let roster = core.author_roster(None).await.expect("roster");
    assert_eq!(roster.authors.len(), 3);
    assert!(
        roster
            .authors
            .iter()
            .any(|author| author.login == "author3" && author.selected)
    );
    let capped = core.author_roster(Some(1)).await.expect("roster");
    assert!(capped.authors.iter().any(|author| author.selected));
    assert_eq!(capped.authors.len() as u32 + capped.hidden, 3);

    let feed = core
        .toggle_collapsed("octo/repo".into())
        .await
        .expect("collapse");
    assert_eq!(feed.repos[0].body, RepoBody::Collapsed);
    assert!(matches!(
        core.toggle_collapsed("no/such".into()).await,
        Err(RostrumError::InvalidInput { .. })
    ));

    let feed = core.clear_filter().await.expect("clear");
    assert!(!feed.filter_active);
    assert!(feed.preferences.hide_empty_repos);

    assert_eq!(core.refresh_feed().await, Err(RostrumError::NotSignedIn));
    assert_eq!(
        core.check_notifications().await,
        Err(RostrumError::NotSignedIn)
    );
    core.mark_notifications_seen().await.expect("seen");
}

#[derive(Default)]
struct Recorder(Mutex<Vec<u64>>);

impl FeedObserver for Recorder {
    fn feed_changed(&self, snapshot: FeedSnapshot) {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(snapshot.revision);
        }
    }
}

impl Recorder {
    async fn wait_for(&self, count: usize) -> Vec<u64> {
        for _ in 0..200 {
            let seen = self.0.lock().expect("lock").clone();
            if seen.len() >= count {
                return seen;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.0.lock().expect("lock").clone()
    }
}

#[tokio::test]
async fn a_feed_observer_sees_every_change_in_order() {
    let scratch = Scratch::new("observer");
    seed(&scratch.dir, &[pull(1, "h1")], None).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");
    core.cached_feed().await.expect("feed");

    let recorder = Arc::new(Recorder::default());
    core.set_feed_observer(Some(recorder.clone()))
        .await
        .expect("observe");
    core.set_query("x".into()).await.expect("query");
    let last = core.set_query(String::new()).await.expect("query");

    let seen = recorder.wait_for(3).await;
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]), "{seen:?}");
    assert_eq!(seen.last(), Some(&last.revision));

    core.set_feed_observer(None).await.expect("stop");
    core.set_query("y".into()).await.expect("query");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(recorder.0.lock().expect("lock").len(), 3);
}

/// The anchor of the first line row matching `kind`.
fn anchor_of(rows: &[DiffRow], kind: LineKind, line: u32) -> CommentAnchor {
    rows.iter()
        .find_map(|row| match row {
            DiffRow::Line { line: diff_line }
                if diff_line.kind == kind
                    && diff_line.anchor.as_ref().is_some_and(|a| a.line == line) =>
            {
                diff_line.anchor.clone()
            }
            _ => None,
        })
        .expect("a commentable line")
}

#[tokio::test]
async fn drafts_anchor_persist_and_go_stale_when_the_head_moves() {
    let scratch = Scratch::new("drafts");
    seed(&scratch.dir, &[pull(1, "head1")], Some("head1")).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");

    let overview = core
        .files_overview("octo/repo".into(), 1)
        .await
        .expect("overview from the cache");
    assert_eq!(overview.head_sha, "head1");
    assert_eq!(overview.files[0].availability, DiffAvailability::Text);
    assert_eq!(overview.stats.additions, 2);

    let diff = core
        .file_diff("octo/repo".into(), 1, 0)
        .await
        .expect("diff");
    let FileDiffBody::Rows { rows } = &diff.body else {
        panic!("rows");
    };
    let added = anchor_of(rows, LineKind::Added, 11);
    assert_eq!(added.side, FfiSide::Right);
    let removed = anchor_of(rows, LineKind::Removed, 11);
    assert_eq!(removed.side, FfiSide::Left);
    let context = anchor_of(rows, LineKind::Context, 10);
    let end = anchor_of(rows, LineKind::Context, 12);

    let review = core
        .add_draft(
            "octo/repo".into(),
            1,
            added.clone(),
            None,
            "Why two?".into(),
        )
        .await
        .expect("single draft");
    assert_eq!(review.drafts.len(), 1);
    assert_eq!(review.drafted_against.as_deref(), Some("head1"));
    assert!(!review.stale);
    let review = core
        .add_draft(
            "octo/repo".into(),
            1,
            end.clone(),
            Some(context.clone()),
            "This block".into(),
        )
        .await
        .expect("range draft");
    assert_eq!(review.drafts[1].anchor.start_line, Some(10));
    assert_eq!(review.drafts[1].anchor.line, 12);
    let review = core
        .add_draft("octo/repo".into(), 1, removed, None, "old line".into())
        .await
        .expect("left draft");
    let removed_draft = review.drafts[2].id;

    // Anchors the diff did not produce are refused.
    for (anchor, start) in [
        (
            CommentAnchor {
                line: 13,
                ..added.clone()
            },
            None,
        ),
        (anchor_of(rows, LineKind::Added, 40), Some(context.clone())),
    ] {
        assert!(matches!(
            core.add_draft("octo/repo".into(), 1, anchor, start, "x".into())
                .await,
            Err(RostrumError::InvalidInput { .. })
        ));
    }

    // Drafts appear in the diff after their (last) line.
    let diff = core
        .file_diff("octo/repo".into(), 1, 0)
        .await
        .expect("diff");
    let FileDiffBody::Rows { rows } = &diff.body else {
        panic!("rows");
    };
    let drafts_in_diff = rows
        .iter()
        .filter(|row| matches!(row, DiffRow::Draft { .. }))
        .count();
    assert_eq!(drafts_in_diff, 3);
    assert_eq!(diff.file.drafts, 3);

    core.remove_draft("octo/repo".into(), 1, removed_draft)
        .await
        .expect("remove");
    let first = review.drafts[0].id;
    core.edit_draft("octo/repo".into(), 1, first, "Why two, not one?".into())
        .await
        .expect("edit");
    assert!(matches!(
        core.edit_draft("octo/repo".into(), 1, 999, "x".into())
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    drop(core);

    // A crash or restart loses nothing.
    let reopened = RostrumCore::open(scratch.path()).await.expect("reopen");
    let review = reopened
        .pending_review("octo/repo".into(), 1)
        .await
        .expect("pending");
    assert_eq!(
        review
            .drafts
            .iter()
            .map(|draft| draft.body.as_str())
            .collect::<Vec<_>>(),
        vec!["Why two, not one?", "This block"]
    );
    assert!(!review.stale);
    drop(reopened);

    // The author force-pushes: the feed now reports a different head.
    let db = Db::open(&scratch.dir.join("cache.db")).await.expect("db");
    db.save_pull_requests(&repo(), &[pull(1, "head2")])
        .await
        .expect("prs");
    db.save_pull_request_files(&repo(), PrNumber(1), "head2", &files())
        .await
        .expect("files");
    db.close().await;

    let moved = RostrumCore::open(scratch.path()).await.expect("reopen");
    let review = moved
        .pending_review("octo/repo".into(), 1)
        .await
        .expect("pending");
    assert!(review.stale);
    assert_eq!(review.drafted_against.as_deref(), Some("head1"));
    assert_eq!(review.head_sha, "head2");

    // Stale drafts are not placed in the new diff, cannot be extended, and
    // cannot be submitted.
    let diff = moved
        .file_diff("octo/repo".into(), 1, 0)
        .await
        .expect("diff");
    let FileDiffBody::Rows { rows } = &diff.body else {
        panic!("rows");
    };
    assert!(!rows.iter().any(|row| matches!(row, DiffRow::Draft { .. })));
    assert!(matches!(
        moved
            .add_draft("octo/repo".into(), 1, added, None, "more".into())
            .await,
        Err(RostrumError::DraftsStale { .. })
    ));
    moved
        .set_github_token(Some("ghp_test".into()))
        .await
        .expect("token");
    assert!(matches!(
        moved
            .submit_review(
                "octo/repo".into(),
                1,
                ReviewEvent::Comment,
                "x".into(),
                true
            )
            .await,
        Err(RostrumError::DraftsStale { .. })
    ));

    let review = moved
        .discard_drafts("octo/repo".into(), 1)
        .await
        .expect("discard");
    assert!(review.drafts.is_empty() && !review.stale);
    assert_eq!(review.drafted_against, None);
}

#[tokio::test]
async fn the_detail_renders_from_the_cache() {
    let scratch = Scratch::new("detail");
    seed(&scratch.dir, &[pull(1, "h1"), pull(2, "h2")], Some("h1")).await;
    let db = Db::open(&scratch.dir.join("cache.db")).await.expect("db");
    let conversation = Conversation {
        items: vec![TimelineItem::Body {
            author: Some(User {
                login: "author1".into(),
                avatar_url: None,
            }),
            body: "Fixes **#9**".into(),
            created_at: support::at(1_700_000_000),
        }],
        threads: vec![ReviewThread {
            id: ThreadId("T1".into()),
            path: "src/lib.rs".into(),
            line: Some(11),
            original_line: Some(11),
            side: Side::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![ThreadComment {
                id: CommentId("C1".into()),
                database_id: Some(5),
                author: None,
                body: "why?".into(),
                created_at: support::at(1_700_000_100),
            }],
        }],
        checks: vec![CheckRun {
            name: "ci".into(),
            state: Some(CheckState::Success),
            url: None,
        }],
        state: Some(CoreState::Merged),
    };
    db.save_conversation(&repo(), PrNumber(1), &conversation)
        .await
        .expect("conversation");
    db.close().await;

    let core = RostrumCore::open(scratch.path()).await.expect("open");
    let detail = core
        .cached_pull_detail("octo/repo".into(), 1)
        .await
        .expect("detail")
        .expect("cached");
    assert_eq!(detail.header.title, "Pull request 1");
    assert_eq!(detail.header.state, PullState::Merged);
    assert_eq!(detail.header.head_sha, "h1");
    assert_eq!(detail.timeline[0].id, "description");
    assert_eq!(detail.threads[0].location, "src/lib.rs:11");
    assert!(detail.threads[0].can_reply);
    assert_eq!(detail.unresolved_threads, 1);
    assert_eq!(detail.checks[0].status_text, "success");
    assert!(detail.pending_review.drafts.is_empty());

    // The diff places the cached thread at its line.
    let diff = core
        .file_diff("octo/repo".into(), 1, 0)
        .await
        .expect("diff");
    let FileDiffBody::Rows { rows } = &diff.body else {
        panic!("rows");
    };
    let thread_at = rows
        .iter()
        .position(|row| matches!(row, DiffRow::Thread { .. }))
        .expect("thread row");
    let DiffRow::Line { line } = &rows[thread_at - 1] else {
        panic!("a thread follows a line");
    };
    assert_eq!((line.new_line, line.kind), (Some(11), LineKind::Added));
    assert_eq!(diff.file.threads, 1);

    let header = core
        .pull_header("octo/repo".into(), 2)
        .await
        .expect("header");
    assert_eq!(header.state, PullState::Open);
    assert_eq!(
        core.cached_pull_detail("octo/repo".into(), 2)
            .await
            .expect("detail"),
        None
    );
    assert_eq!(
        core.pull_header("octo/repo".into(), 404).await,
        Err(RostrumError::UnknownPullRequest {
            repo: "octo/repo".into(),
            number: 404
        })
    );
    assert_eq!(
        core.pull_detail("octo/repo".into(), 1).await,
        Err(RostrumError::NotSignedIn)
    );
    assert!(matches!(
        core.file_diff("octo/repo".into(), 1, 7).await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.pull_header("octo/repo".into(), 0).await,
        Err(RostrumError::InvalidInput { .. })
    ));
    // A reply is looked up in the cached conversation; without a token it
    // stops before the network.
    assert_eq!(
        core.reply_to_thread("octo/repo".into(), 1, "T1".into(), "because".into())
            .await,
        Err(RostrumError::NotSignedIn)
    );
    assert!(matches!(
        core.add_comment("octo/repo".into(), 1, "  ".into()).await,
        Err(RostrumError::InvalidInput { .. })
    ));
}
